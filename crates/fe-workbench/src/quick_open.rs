//! Quick open (Cmd-P): a [`Picker`] over the project's [`FinderIndex`].
//!
//! The index is built the first time the palette opens with a root, so it
//! costs startup nothing: [`FinderIndex::walk`] runs in the background and
//! queries see what it has indexed so far. It's kept for the workbench's
//! life and kept current with the project panel's watcher batches.
//!
//! The index sits behind a mutex, and every query, walk batch and watcher
//! batch reaches it off the main thread. One query runs at a time: one
//! typed meanwhile waits in its place, replacing any already waiting, and
//! results for text the input no longer holds are dropped. Until the input's
//! text has results, the rows shown are another query's, and picking one is
//! refused; opening clears them.
//!
//! A quick open made to pick a file for something else ([`QuickOpen::set_pick`])
//! hands the file picked to it instead of opening it.
//!
//! A query starting with `@` goes to the symbol picker, the rest as its query
//! ([`QuickOpen::hand_symbols_to`]), as VS Code's quick open does.

use std::{
    fmt::Write as _,
    mem,
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_project::{FinderIndex, FinderMatch, FsChange};
use fe_ui::{HighlightedLabel, char_ranges};
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, Context, FocusHandle, IntoElement as _,
    SharedString, Task, WeakEntity, Window,
};
use kiln_l10n::tr;

use crate::{
    Pane, Workbench,
    pane::Reveal,
    picker::{Picker, PickerDelegate},
};

/// The most matches a query shows.
pub(crate) const LIMIT: usize = 50;

/// What picking a file does in place of opening it: see
/// [`QuickOpen::set_pick`].
pub(crate) type Pick = Box<dyn FnOnce(PathBuf, &mut Window, &mut App)>;

/// `None` until the walk's thread has created it.
type SharedIndex = Arc<Mutex<Option<FinderIndex>>>;

enum Build {
    /// The palette hasn't opened with a root yet.
    NotStarted,
    Walking {
        _walk: Task<()>,
    },
    Done,
}

pub struct QuickOpen {
    root: Option<PathBuf>,
    pane: WeakEntity<Pane>,
    index: SharedIndex,
    build: Build,
    /// Files indexed so far, from the walk's batches.
    indexed: usize,
    /// Watcher batches to apply: held while the walk runs, then applied
    /// one apply at a time.
    held: Vec<FsChange>,
    applying: Option<Task<()>>,
    /// The input's text, as last given to `update_matches`.
    query: String,
    /// What `matches` are for: `None` once cleared on opening.
    shown: Option<String>,
    matches: Vec<FinderMatch>,
    /// The query to run once the running one is done.
    pending: Option<String>,
    querying: Option<Task<()>>,
    /// Focused before the palette opened, and again once it's dismissed.
    previous_focus: Option<FocusHandle>,
    /// Stops the walk when the workbench goes.
    cancel: Arc<AtomicBool>,
    /// Takes the file picked, if set, in place of opening it.
    pick: Option<Pick>,
    /// Takes an `@` query, if set, and the window it's shown in.
    symbols: Option<(WeakEntity<Workbench>, AnyWindowHandle)>,
    #[cfg(test)]
    pub(crate) walks: usize,
    #[cfg(test)]
    pub(crate) queries: usize,
}

impl QuickOpen {
    /// Opens paths under `root` in `pane`. Touches nothing until opened.
    pub fn new(root: Option<PathBuf>, pane: WeakEntity<Pane>) -> Self {
        Self {
            root,
            pane,
            index: SharedIndex::default(),
            build: Build::NotStarted,
            indexed: 0,
            held: Vec::new(),
            applying: None,
            query: String::new(),
            shown: None,
            matches: Vec::new(),
            pending: None,
            querying: None,
            previous_focus: None,
            cancel: Arc::default(),
            pick: None,
            symbols: None,
            #[cfg(test)]
            walks: 0,
            #[cfg(test)]
            queries: 0,
        }
    }

    /// The palette is opening, with `previous_focus` focused until now.
    /// Clears the last opening's rows, and starts building the index the
    /// first time.
    pub fn opened(&mut self, previous_focus: Option<FocusHandle>, cx: &mut Context<Picker<Self>>) {
        self.previous_focus = previous_focus;
        self.matches.clear();
        self.shown = None;
        if matches!(self.build, Build::NotStarted) {
            self.walk(cx);
        }
    }

    /// Hands the next file picked to `pick` rather than opening it.
    pub(crate) fn set_pick(&mut self, pick: Pick) {
        self.pick = Some(pick);
    }

    /// Hands `@` queries to `workbench`'s symbol picker, in `window`.
    pub(crate) fn hand_symbols_to(
        &mut self,
        workbench: WeakEntity<Workbench>,
        window: AnyWindowHandle,
    ) {
        self.symbols = Some((workbench, window));
    }

    /// Opens paths in `pane` from now on: the active one, as it changes.
    pub(crate) fn set_pane(&mut self, pane: WeakEntity<Pane>) {
        self.pane = pane;
    }

    /// The row for match `ix`: its path, the matched chars highlighted.
    pub fn label(&self, ix: usize) -> Option<HighlightedLabel> {
        let found = self.matches.get(ix)?;
        let highlights = char_ranges(&found.path, &found.indices);
        // Coloured by the active theme as it draws.
        Some(HighlightedLabel {
            text: found.path.clone().into(),
            highlights,
        })
    }

    pub fn matches(&self) -> &[FinderMatch] {
        &self.matches
    }

    /// A watcher batch, as the project panel got it. Before the index
    /// exists it's dropped, since the walk will see the disk as it is.
    pub fn fs_changed(&mut self, changes: &[FsChange], cx: &mut Context<Picker<Self>>) {
        match self.build {
            Build::NotStarted => {}
            Build::Walking { .. } => self.held.extend_from_slice(changes),
            Build::Done => {
                self.held.extend_from_slice(changes);
                self.apply_held(cx);
            }
        }
    }

    fn walk(&mut self, cx: &mut Context<Picker<Self>>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        #[cfg(test)]
        {
            self.walks += 1;
        }
        let index = self.index.clone();
        let cancel = self.cancel.clone();
        let (sender, batches) = async_channel::unbounded();
        let walk = cx.background_spawn(async move {
            *lock(&index) = Some(FinderIndex::new(&root));
            FinderIndex::walk(&root, cancel, |batch| {
                let mut index = lock(&index);
                let index = index.as_mut().expect("created above");
                index.extend(batch);
                sender.send_blocking(index.progress().0).ok();
            });
            lock(&index).as_mut().expect("created above").finish();
        });
        let walked = cx.spawn(async move |picker, cx| {
            // The channel closes once the walk is done.
            while let Ok(indexed) = batches.recv().await {
                let updated = picker.update(cx, |picker, cx| {
                    picker.delegate.indexed = indexed;
                    picker.delegate.requery(cx);
                });
                if updated.is_err() {
                    return;
                }
            }
            walk.await;
            picker
                .update(cx, |picker, cx| {
                    let quick_open = &mut picker.delegate;
                    quick_open.build = Build::Done;
                    quick_open.apply_held(cx);
                    quick_open.requery(cx);
                })
                .ok();
        });
        self.build = Build::Walking { _walk: walked };
    }

    /// Applies the held watcher batches in the background, unless an apply
    /// is running: they wait for it, so batches apply in order.
    fn apply_held(&mut self, cx: &mut Context<Picker<Self>>) {
        if self.applying.is_some() || self.held.is_empty() {
            return;
        }
        let changes = mem::take(&mut self.held);
        let index = self.index.clone();
        let apply = cx.background_spawn(async move {
            if let Some(index) = lock(&index).as_mut() {
                index.apply(&changes);
            }
        });
        self.applying = Some(cx.spawn(async move |picker, cx| {
            apply.await;
            picker
                .update(cx, |picker, cx| {
                    let quick_open = &mut picker.delegate;
                    quick_open.applying = None;
                    quick_open.apply_held(cx);
                    quick_open.requery(cx);
                })
                .ok();
        }));
    }

    /// Matches the input's text again, as the index changed.
    fn requery(&mut self, cx: &mut Context<Picker<Self>>) {
        self.update_matches(self.query.clone(), cx);
    }

    /// Runs the waiting query in the background.
    fn run_query(&mut self, cx: &mut Context<Picker<Self>>) {
        let Some(query) = self.pending.take() else {
            return;
        };
        #[cfg(test)]
        {
            self.queries += 1;
        }
        let index = self.index.clone();
        let search = cx.background_spawn({
            let query = query.clone();
            async move {
                lock(&index).as_mut().map_or_else(Vec::new, |index| {
                    index.query(split_location(&query).0, LIMIT)
                })
            }
        });
        self.querying = Some(cx.spawn(async move |picker, cx| {
            let matches = search.await;
            picker
                .update(cx, |picker, cx| {
                    let quick_open = &mut picker.delegate;
                    quick_open.querying = None;
                    if quick_open.query == query {
                        let new_query = quick_open.shown.as_ref() != Some(&query);
                        quick_open.shown = Some(query);
                        quick_open.matches = matches;
                        picker.matches_updated(new_query, cx);
                    }
                    picker.delegate.run_query(cx);
                })
                .ok();
        }));
    }

    #[cfg(test)]
    pub(crate) fn index_progress(&self) -> Option<(usize, bool)> {
        lock(&self.index).as_ref().map(FinderIndex::progress)
    }

    #[cfg(test)]
    pub(crate) fn index_stats(&self) -> Option<fe_project::FinderStats> {
        lock(&self.index).as_ref().map(FinderIndex::stats)
    }

    #[cfg(test)]
    pub(crate) fn status_text(&self) -> String {
        self.status().unwrap_or_default().to_string()
    }

    /// Shows `matches` as the input's, as a query's results would.
    #[cfg(all(test, unix))]
    pub(crate) fn show_matches(&mut self, matches: Vec<FinderMatch>) {
        self.shown = Some(self.query.clone());
        self.matches = matches;
    }

    #[cfg(test)]
    pub(crate) fn held(&self) -> usize {
        self.held.len()
    }
}

impl PickerDelegate for QuickOpen {
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>) {
        // A file open with no folder has symbols too; picking a file for
        // something else has no use for them.
        if let Some(symbols) = query.strip_prefix('@')
            && self.pick.is_none()
            && let Some((workbench, window)) = self.symbols.clone()
        {
            let symbols = symbols.to_owned();
            // The picker is being updated now.
            cx.defer(move |cx| {
                let shown = window.update(cx, |_, window, cx| {
                    workbench.update(cx, |workbench, cx| {
                        workbench.show_document_symbols(&symbols, window, cx)
                    })
                });
                drop(shown);
            });
            return;
        }
        if self.root.is_none() {
            return;
        }
        self.query = query.clone();
        self.pending = Some(query);
        if self.querying.is_none() {
            self.run_query(cx);
        }
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    /// The path on disk, losslessly, as `path` is lossy: as is if it's
    /// UTF-8, else a NUL, which no path holds, then its bytes in hex.
    fn match_id(&self, ix: usize) -> SharedString {
        let relative = self.matches[ix].relative.as_os_str();
        if let Some(path) = relative.to_str() {
            return path.to_owned().into();
        }
        let mut id = String::from("\0");
        for byte in relative.as_encoded_bytes() {
            write!(id, "{byte:02x}").expect("writing to a String");
        }
        id.into()
    }

    fn render_match(&self, ix: usize, _: bool, _: &App) -> AnyElement {
        self.label(ix).map_or_else(
            || gpui::Empty.into_any_element(),
            |label| label.into_any_element(),
        )
    }

    /// Opens the file as a tree click does: in a focused tab, or by
    /// activating its tab if it's open; or hands it to the pick set. Refused
    /// while the rows are another query's than the input's.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let (Some(root), Some(found)) = (&self.root, self.matches.get(ix)) else {
            return false;
        };
        if self.shown.as_ref() != Some(&self.query) {
            return false;
        }
        let path = root.join(&found.relative);
        if let Some(pick) = self.pick.take() {
            self.previous_focus = None;
            pick(path, window, cx);
            return true;
        }
        let reveal = split_location(&self.query).1.map(|(row, column)| Reveal {
            row,
            columns: column..column,
            within_row: true,
        });
        self.previous_focus = None;
        self.pane
            .update(cx, |pane, cx| pane.open_at(path, reveal, window, cx))
            .is_ok()
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        Some(match (&self.root, &self.build) {
            (None, _) => tr!("workbench-navigation-no-folder"),
            (Some(_), Build::Done) => tr!(
                "workbench-navigation-results",
                count = self.matches.len(),
                source = "exact",
                provider = ""
            ),
            (Some(_), _) => tr!("workbench-navigation-indexing", count = self.indexed),
        })
    }
}

impl Drop for QuickOpen {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Splits a trailing `:N` or `:N:C` (digits, both at least 1) off `query`:
/// the rest, and the 0-based row and byte column to open at. Anything else,
/// or nothing before the suffix, is the query whole.
fn split_location(query: &str) -> (&str, Option<(u32, usize)>) {
    fn number(text: &str) -> Option<u32> {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        // Past u32 is still a number: the location clamps to the file.
        Some(text.parse().unwrap_or(u32::MAX)).filter(|&number| number >= 1)
    }
    let whole = (query, None);
    let Some((rest, last)) = query.rsplit_once(':') else {
        return whole;
    };
    let Some(last) = number(last) else {
        return whole;
    };
    if let Some((path, row)) = rest.rsplit_once(':')
        && let Some(row) = number(row)
        && !path.is_empty()
    {
        return (path, Some((row - 1, last as usize - 1)));
    }
    if rest.is_empty() {
        return whole;
    }
    (rest, Some((last - 1, 0)))
}

fn lock(index: &Mutex<Option<FinderIndex>>) -> MutexGuard<'_, Option<FinderIndex>> {
    index.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
#[path = "quick_open_line_tests.rs"]
mod line_tests;

#[cfg(test)]
#[path = "quick_open_at_prefix_tests.rs"]
mod quick_open_at_prefix_tests;
