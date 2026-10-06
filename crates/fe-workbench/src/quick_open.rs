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
use fe_ui::{HighlightedLabel, UiTheme, char_ranges};
use gpui::{
    AnyElement, App, AppContext as _, Context, FocusHandle, IntoElement as _, SharedString, Task,
    WeakEntity, Window,
};

use crate::{
    Pane,
    picker::{Picker, PickerDelegate},
};

/// The most matches a query shows.
pub(crate) const LIMIT: usize = 50;

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
    ui: UiTheme,
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
    #[cfg(test)]
    pub(crate) walks: usize,
    #[cfg(test)]
    pub(crate) queries: usize,
}

impl QuickOpen {
    /// Opens paths under `root` in `pane`. Touches nothing until opened.
    pub fn new(root: Option<PathBuf>, pane: WeakEntity<Pane>, ui: UiTheme) -> Self {
        Self {
            root,
            pane,
            ui,
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

    /// Opens paths in `pane` from now on: the active one, as it changes.
    pub(crate) fn set_pane(&mut self, pane: WeakEntity<Pane>) {
        self.pane = pane;
    }

    /// The row for match `ix`: its path, the matched chars highlighted.
    pub fn label(&self, ix: usize) -> Option<HighlightedLabel> {
        let found = self.matches.get(ix)?;
        let highlights = char_ranges(&found.path, &found.indices);
        Some(HighlightedLabel::new(
            found.path.clone(),
            highlights,
            &self.ui,
        ))
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
                lock(&index)
                    .as_mut()
                    .map_or_else(Vec::new, |index| index.query(&query, LIMIT))
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
    #[cfg(test)]
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
    /// activating its tab if it's open. Refused while the rows are another
    /// query's than the input's.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool {
        let (Some(root), Some(found)) = (&self.root, self.matches.get(ix)) else {
            return false;
        };
        if self.shown.as_ref() != Some(&self.query) {
            return false;
        }
        let path = root.join(&found.relative);
        self.previous_focus = None;
        self.pane
            .update(cx, |pane, cx| pane.open(path, window, cx))
            .is_ok()
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
    }

    fn status(&self) -> Option<SharedString> {
        Some(match (&self.root, &self.build) {
            (None, _) => "No folder open".into(),
            (Some(_), Build::Done) if self.matches.len() == 1 => "1 result".into(),
            (Some(_), Build::Done) => format!("{} results", self.matches.len()).into(),
            (Some(_), _) => format!("Indexing… {} files", self.indexed).into(),
        })
    }
}

impl Drop for QuickOpen {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn lock(index: &Mutex<Option<FinderIndex>>) -> MutexGuard<'_, Option<FinderIndex>> {
    index.lock().unwrap_or_else(PoisonError::into_inner)
}
