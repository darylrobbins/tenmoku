//! Go to definition (F12), workspace symbols (Cmd-T), back and forward:
//! `fe-nav`'s [`Navigation`] behind the workbench's `nav.*` commands.
//!
//! The workbench owns one [`Navigation`]; a host adds providers to it
//! ([`Workbench::navigation`]). Its search provider, [`SearchNavigation`],
//! answers from a [`SymbolIndex`] built the first time a nav command runs
//! with a root, so it costs startup nothing: [`fe_nav::build`] runs in the
//! background, and until it's done go-to-definition answers by search, with
//! a capped text search. The index is kept current with the project panel's
//! watcher batches, held while it builds, then planned, extracted and
//! updated in the background one batch at a time.
//!
//! Requests read the active editor: its file relative to the root, its
//! language, its text and its primary selection's head (the one scrolling
//! follows), and the buffer and version the text is, which a language
//! server's provider checks against. They run on the background executor
//! once the search provider is in. Each caller has its own [`Requests`]: a
//! new request cancels its caller's last, whose answer is dropped if it
//! lands.
//!
//! A jump first records where the caret was in [`NavHistory`], which back
//! and forward walk. A location in an open buffer's version goes to that
//! buffer, mapped onto it as it is ([`Workbench::target_of`]).

use std::{
    mem,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard,
        atomic::{AtomicBool, Ordering},
    },
};

use async_channel::{Receiver, Sender};
use fe_buffer::Bias;
use fe_nav::{
    Answer, BuildOptions, Definitions, Location, MAX_BATCH_FILES, NavError, NavRequest, Navigation,
    Precision, Related, SearchNavigation, SymbolIndex,
};
use fe_project::FsChange;
use gpui::{App, AppContext as _, Context, Entity, Focusable as _, Task, Window};
use kiln_l10n::{Text, text, tr, verbatim};

use crate::{
    Picker, StatusBar, StatusItem, StatusSide, Workbench,
    nav_picker::{DefinitionsPicker, SymbolsPicker},
    pane::Reveal,
    pane_toolbar::NavButtons,
    workbench::Palette,
};

/// The most places [`NavHistory`] holds, back and forward together.
pub(crate) const HISTORY_LIMIT: usize = 100;

/// The status bar item naming the provider of the last answer.
pub(crate) const SOURCE_ITEM: &str = verbatim!("nav_source");

/// A place to go back or forward to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) path: PathBuf,
    /// A byte offset in the file's text.
    pub(crate) offset: usize,
}

/// Where jumps came from, to go back to, and where going back left, to go
/// forward to again. A jump clears the way forward.
#[derive(Debug, Default)]
pub(crate) struct NavHistory {
    back: Vec<Place>,
    forward: Vec<Place>,
}

impl NavHistory {
    /// Records `place`, if there is one, before a jump, dropping the oldest
    /// past the limit. Clears the way forward either way.
    pub(crate) fn push(&mut self, place: Option<Place>) {
        self.forward.clear();
        let Some(place) = place else {
            return;
        };
        if self.back.len() == HISTORY_LIMIT {
            self.back.remove(0);
        }
        self.back.push(place);
    }

    /// The place to go back to from `here`, which forward then returns to.
    pub(crate) fn back(&mut self, here: Option<Place>) -> Option<Place> {
        let to = self.back.pop()?;
        self.forward.extend(here);
        Some(to)
    }

    /// The place to go forward to from `here`, which back then returns to.
    pub(crate) fn forward(&mut self, here: Option<Place>) -> Option<Place> {
        let to = self.forward.pop()?;
        self.back.extend(here);
        Some(to)
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.back.len() + self.forward.len()
    }

    fn buttons(&self) -> NavButtons {
        NavButtons {
            back: !self.back.is_empty(),
            forward: !self.forward.is_empty(),
        }
    }
}

/// One caller's requests: a new one cancels the last, and only the
/// newest's answer lands.
#[derive(Debug, Default)]
pub(crate) struct Requests {
    generation: u64,
    cancel: Arc<AtomicBool>,
}

impl Requests {
    /// The newest request's cancel flag: set once a newer one starts.
    pub(crate) fn current(&self) -> &Arc<AtomicBool> {
        &self.cancel
    }

    pub(crate) fn next(&mut self) -> (u64, Arc<AtomicBool>) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::default();
        self.generation += 1;
        (self.generation, self.cancel.clone())
    }
}

enum Build {
    /// No nav command has run with this root yet. Dropping the sender
    /// closes [`Nav::ready`].
    NotStarted(Sender<()>),
    Running {
        _build: Task<()>,
    },
    Done,
}

/// The workbench's navigation state, for one root.
pub(crate) struct Nav {
    root: Option<PathBuf>,
    navigation: Arc<Navigation>,
    /// Closed once the search provider for `root` is in.
    ready: Receiver<()>,
    build: Build,
    /// Set once the build has created it.
    index: Option<Arc<RwLock<SymbolIndex>>>,
    /// Stops the build and the reindexing when the root goes.
    cancel: Arc<AtomicBool>,
    /// Watcher batches to apply: held while the build runs, then applied
    /// one batch at a time, in order.
    held: Vec<FsChange>,
    applying: Option<Task<()>>,
    pub(crate) history: NavHistory,
    pub(crate) goto: Requests,
    /// While a peek is asked for, intercepts Esc in its editor, which cancels it
    /// ([`Workbench::goto_definition`]).
    pub(crate) peek_escape: Option<gpui::Subscription>,
    pub(crate) definitions: Entity<Picker<DefinitionsPicker>>,
    pub(crate) symbols: Entity<Picker<SymbolsPicker>>,
    /// Builds started.
    #[cfg(test)]
    pub(crate) builds: usize,
    /// Awaited before the next build walks.
    #[cfg(test)]
    pub(crate) build_gate: Option<Receiver<()>>,
    /// Awaited before the next request's answer lands.
    #[cfg(test)]
    pub(crate) hold_answer: Option<Receiver<()>>,
    /// Each request's cancel flag, oldest first.
    #[cfg(test)]
    pub(crate) cancels: Vec<Arc<AtomicBool>>,
}

impl Nav {
    /// Navigation in `root`, built on the first nav command.
    pub(crate) fn new(root: Option<PathBuf>, cx: &mut Context<Workbench>) -> Self {
        let navigation = Arc::new(Navigation::new());
        let workbench = cx.weak_entity();
        let definitions = cx.new(|cx| {
            let delegate = DefinitionsPicker::new(workbench.clone());
            let placeholder = text!("workbench-navigation-definitions-placeholder");
            Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx)
        });
        let symbols = cx.new(|cx| {
            let delegate = SymbolsPicker::new(workbench, navigation.clone());
            let placeholder = text!("workbench-navigation-symbols-placeholder");
            Picker::new(delegate, placeholder, fe_ui::ui_theme(cx), cx)
        });
        let (ready_tx, ready) = async_channel::bounded(1);
        Self {
            root,
            navigation,
            ready,
            build: Build::NotStarted(ready_tx),
            index: None,
            cancel: Arc::default(),
            held: Vec::new(),
            applying: None,
            history: NavHistory::default(),
            goto: Requests::default(),
            peek_escape: None,
            definitions,
            symbols,
            #[cfg(test)]
            builds: 0,
            #[cfg(test)]
            build_gate: None,
            #[cfg(test)]
            hold_answer: None,
            #[cfg(test)]
            cancels: Vec::new(),
        }
    }

    pub(crate) fn navigation(&self) -> &Arc<Navigation> {
        &self.navigation
    }

    /// The index's files and whether it's complete, once it exists.
    #[cfg(test)]
    pub(crate) fn index_progress(&self) -> Option<(usize, bool)> {
        self.index.as_ref().map(|index| read(index).progress())
    }

    /// Navigates in `root` from now on: the old root's build stops and its
    /// search provider goes, a new one being built on the next nav command.
    /// Requests in flight are cancelled; the history stays.
    pub(crate) fn set_root(&mut self, root: PathBuf, cx: &mut Context<Workbench>) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::default();
        self.navigation.remove(SearchNavigation::NAME);
        let (ready_tx, ready) = async_channel::bounded(1);
        self.ready = ready;
        self.build = Build::NotStarted(ready_tx);
        self.index = None;
        self.held.clear();
        self.applying = None;
        self.goto.next();
        self.root = Some(root);
        self.symbols.update(cx, |picker, cx| {
            picker.delegate.reset();
            cx.notify();
        });
    }

    /// Starts building the index the first time, and gives what closes once
    /// its provider is in; `None` without a root.
    pub(crate) fn start(&mut self, cx: &mut Context<Workbench>) -> Option<Receiver<()>> {
        let root = self.root.clone()?;
        if let Build::NotStarted(_) = self.build {
            let Build::NotStarted(ready) = mem::replace(&mut self.build, Build::Done) else {
                unreachable!("matched above")
            };
            self.build = Build::Running {
                _build: self.spawn_build(root, ready, cx),
            };
            self.symbols
                .update(cx, |picker, _| picker.delegate.indexing(Some(0)));
        }
        Some(self.ready.clone())
    }

    fn spawn_build(
        &mut self,
        root: PathBuf,
        ready: Sender<()>,
        cx: &mut Context<Workbench>,
    ) -> Task<()> {
        #[cfg(test)]
        let gate = {
            self.builds += 1;
            self.build_gate.take()
        };
        let navigation = self.navigation.clone();
        let cancel = self.cancel.clone();
        cx.spawn(async move |workbench, cx| {
            // `SymbolIndex::new` resolves the root on disk.
            let index = cx
                .background_spawn({
                    let root = root.clone();
                    async move { Arc::new(RwLock::new(SymbolIndex::new(root))) }
                })
                .await;
            navigation.add(Arc::new(search_provider(&root, &index)), 0);
            drop(ready);
            let index_ = index.clone();
            let stored = workbench.update(cx, |workbench, _| {
                workbench.nav_state.index = Some(index_);
            });
            if stored.is_err() {
                return;
            }
            #[cfg(test)]
            if let Some(gate) = gate {
                gate.recv().await.ok();
            }
            let (sender, batches) = async_channel::unbounded();
            let build = cx.background_spawn(async move {
                fe_nav::build(&root, &BuildOptions::default(), cancel.clone(), |batch| {
                    let mut index = write(&index);
                    index.extend(batch);
                    sender.send_blocking(index.progress().0).ok();
                });
                let reindex = write(&index).finish();
                reextract(&index, &reindex.extract, &cancel);
            });
            // The channel closes once the build is done.
            while let Ok(indexed) = batches.recv().await {
                let updated = workbench.update(cx, |workbench, cx| {
                    let shows = workbench.palette == Some(Palette::Symbols);
                    workbench.nav_state.progress(Some(indexed), shows, cx)
                });
                if updated.is_err() {
                    return;
                }
            }
            build.await;
            workbench
                .update(cx, |workbench, cx| {
                    let shows = workbench.palette == Some(Palette::Symbols);
                    let nav = &mut workbench.nav_state;
                    nav.build = Build::Done;
                    nav.apply_held(cx);
                    nav.progress(None, shows, cx);
                })
                .ok();
        })
    }

    /// Tells the symbols picker how many files are indexed, `None` once the
    /// build is done, for it to match its query again if it `shows`.
    fn progress(&mut self, indexed: Option<usize>, shows: bool, cx: &mut Context<Workbench>) {
        self.symbols.update(cx, |picker, cx| {
            picker.delegate.indexing(indexed);
            if shows {
                picker.delegate.requery(cx);
            }
        });
    }

    /// A watcher batch, as the project panel got it: held while the index
    /// builds. Before the index exists it's dropped, since the build will
    /// see the disk as it is.
    pub(crate) fn fs_changed(&mut self, changes: &[FsChange], cx: &mut Context<Workbench>) {
        crate::ignore_diagnostics::tree_changed(changes, cx);
        if self.index.is_none() {
            return;
        }
        self.held.extend_from_slice(changes);
        if let Build::Done = self.build {
            self.apply_held(cx);
        }
    }

    /// Plans the held batches in the background, then extracts the files
    /// the plan names without the lock and updates the index with them,
    /// unless an apply is running: they wait for it, so they apply in order.
    fn apply_held(&mut self, cx: &mut Context<Workbench>) {
        if self.applying.is_some() || self.held.is_empty() {
            return;
        }
        let Some(index) = self.index.clone() else {
            return self.held.clear();
        };
        let changes = mem::take(&mut self.held);
        let cancel = self.cancel.clone();
        let apply = cx.background_spawn(async move {
            let reindex = write(&index).plan(&changes);
            reextract(&index, &reindex.extract, &cancel);
        });
        self.applying = Some(cx.spawn(async move |workbench, cx| {
            apply.await;
            workbench
                .update(cx, |workbench, cx| {
                    let shows = workbench.palette == Some(Palette::Symbols);
                    let nav = &mut workbench.nav_state;
                    nav.applying = None;
                    nav.apply_held(cx);
                    nav.progress(None, shows, cx);
                })
                .ok();
        }));
    }
}

impl Drop for Nav {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.goto.cancel.store(true, Ordering::Relaxed);
    }
}

/// The search provider for `root`, answering from `index`.
fn search_provider(root: &std::path::Path, index: &Arc<RwLock<SymbolIndex>>) -> SearchNavigation {
    let provider = SearchNavigation::new(root, index.clone());
    // A loaded test machine would otherwise cut the fallback short.
    #[cfg(test)]
    let provider = provider.with_budget(std::time::Duration::from_secs(20));
    provider
}

/// Extracts `paths` from the index's root, without its lock, and updates
/// the index with them, a chunk at a time.
fn reextract(index: &RwLock<SymbolIndex>, paths: &[PathBuf], cancel: &AtomicBool) {
    if paths.is_empty() {
        return;
    }
    let root = read(index).canonical_root().to_owned();
    for chunk in paths.chunks(MAX_BATCH_FILES) {
        let extracted = fe_nav::extract_all(&root, chunk, 0, cancel);
        write(index).update(extracted);
    }
}

fn read(index: &RwLock<SymbolIndex>) -> RwLockReadGuard<'_, SymbolIndex> {
    index.read().unwrap_or_else(PoisonError::into_inner)
}

fn write(index: &RwLock<SymbolIndex>) -> RwLockWriteGuard<'_, SymbolIndex> {
    index.write().unwrap_or_else(PoisonError::into_inner)
}

/// Shows on the status bar which provider gave the last answer.
pub(crate) fn show_source(status_bar: &Entity<StatusBar>, provider: &str, cx: &mut App) {
    let item = StatusItem {
        id: SOURCE_ITEM.into(),
        side: StatusSide::Left,
        priority: 100,
        text: tr!("workbench-navigation-source", provider = provider),
        command: None,
    };
    status_bar.update(cx, |bar, cx| bar.set_item(item, cx));
}

/// How a provider answered, as the nav statuses' `source` argument: `search`
/// for a search's answer, which a status names its `provider` after, or
/// `exact`.
pub(crate) fn source(precision: Precision) -> &'static str {
    match precision {
        Precision::Exact => "exact",
        Precision::Search => "search",
    }
}

impl Workbench {
    /// The workbench's navigation, for a host to add providers to (a
    /// language server's, say). Its search provider is added on the first
    /// nav command with a root, at precedence 0.
    pub fn navigation(&self) -> &Arc<Navigation> {
        self.nav_state.navigation()
    }

    /// A request for the active editor's caret, if its file is under the
    /// root.
    pub(crate) fn nav_request(&self, cx: &App) -> Option<NavRequest> {
        let root = self.nav_state.root.as_ref()?;
        let editor = self.active_editor(cx)?.read(cx);
        let model = editor.model();
        let rel = model.path()?.strip_prefix(root).ok()?.to_owned();
        // The text and its version come from the same snapshot.
        let snapshot = model.snapshot();
        Some(NavRequest {
            root: root.clone(),
            rel,
            language: editor.syntax().map(|syntax| syntax.language().name()),
            text: snapshot.text().clone(),
            offset: model.primary_selection().head(),
            buffer: Some((snapshot.id(), snapshot.version())),
        })
    }

    /// Runs `work` on the background executor once the search provider is
    /// in, as a new request of the caller whose [`Requests`] `requests`
    /// picks, cancelling that caller's last. `landed` gets the answer unless
    /// a newer request of the caller's started meanwhile. False without a
    /// root.
    pub(crate) fn run_nav<T: Send + 'static>(
        &mut self,
        requests: fn(&mut Nav) -> &mut Requests,
        work: impl FnOnce(&Navigation, &AtomicBool) -> T + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
        landed: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) -> bool {
        let Some(ready) = self.nav_state.start(cx) else {
            return false;
        };
        let (generation, cancel) = requests(&mut self.nav_state).next();
        #[cfg(test)]
        let hold = {
            self.nav_state.cancels.push(cancel.clone());
            self.nav_state.hold_answer.take()
        };
        let navigation = self.nav_state.navigation.clone();
        cx.spawn_in(window, async move |workbench, cx| {
            ready.recv().await.ok();
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let answer = cx
                .background_spawn(async move { work(&navigation, &cancel) })
                .await;
            #[cfg(test)]
            if let Some(hold) = hold {
                hold.recv().await.ok();
            }
            workbench
                .update_in(cx, |workbench, window, cx| {
                    if requests(&mut workbench.nav_state).generation == generation {
                        landed(workbench, answer, window, cx);
                    }
                })
                .ok();
        })
        .detach();
        true
    }

    /// Records where the active editor's caret is, before a jump.
    pub(crate) fn push_history(&mut self, cx: &mut Context<Self>) {
        let place = self.caret_place(cx);
        self.push_place(place, cx);
    }

    /// Records where `editor`'s caret jumped from, `from` in its file, as
    /// a click on a sticky scroll row does.
    pub(crate) fn editor_jumped(
        &mut self,
        editor: &Entity<fe_gpui::EditorView>,
        from: usize,
        cx: &mut Context<Self>,
    ) {
        let path = editor.read(cx).model().path().map(Path::to_owned);
        let place = path.map(|path| Place { path, offset: from });
        self.push_place(place, cx);
    }

    /// Records `place` before a jump.
    fn push_place(&mut self, place: Option<Place>, cx: &mut Context<Self>) {
        self.nav_state.history.push(place);
        self.set_nav_buttons(self.nav_state.history.buttons(), cx);
    }

    /// Where the active editor's caret is; while the active pane is still
    /// reading a file, where the caret will be in it.
    pub(crate) fn caret_place(&self, cx: &App) -> Option<Place> {
        if let Some(editor) = self.active_editor(cx) {
            let model = editor.read(cx).model();
            return Some(Place {
                path: model.path()?.to_owned(),
                offset: model.primary_selection().head(),
            });
        }
        self.pane().read(cx).pending_place()
    }

    /// Opens `path` in the active pane, focused, with `range` (bytes in its
    /// text) selected once its editor exists.
    pub(crate) fn open_range(
        &mut self,
        path: PathBuf,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reveal = Some(Reveal::bytes(range));
        self.pane()
            .clone()
            .update(cx, |pane, cx| pane.open_at(path, reveal, window, cx));
    }

    /// Jumps to `location`, recording where the caret is first. A
    /// go-to-definition still running is cancelled: its answer would take
    /// the user elsewhere.
    pub(crate) fn nav_jump(
        &mut self,
        location: &Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.nav_state.goto.next();
        self.push_history(cx);
        self.open_location(location, window, cx);
    }

    /// Jumps to `location` from a list that stays open, as find references'
    /// does, recording where the caret is if `record`, and unless it was the
    /// last place recorded. The list says not to where one of its own jumps
    /// left the caret: going through the list, then back, returns at once to
    /// where the caret was before.
    pub(crate) fn list_jump(
        &mut self,
        record: bool,
        location: &Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.nav_state.goto.next();
        let last = self.nav_state.history.back.last();
        let here = (self.caret_place(cx)).filter(|here| record && last != Some(here));
        self.jump_from(here, location, window, cx);
    }

    /// Jumps to `location`, recording `from` first.
    fn jump_from(
        &mut self,
        from: Option<Place>,
        location: &Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.push_place(from, cx);
        self.open_location(location, window, cx);
    }

    /// Opens `location` in the active pane, where it is now
    /// ([`Workbench::target_of`]).
    fn open_location(&mut self, location: &Location, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, reveal)) = self.target_of(location, cx) else {
            return;
        };
        self.pane()
            .clone()
            .update(cx, |pane, cx| pane.open_at(path, Some(reveal), window, cx));
    }

    /// The file to open for `location`, and what to select in it.
    ///
    /// A range in an open buffer's version (a language server's answer,
    /// [`Location::snapshot`]) is that buffer's: it opens by the path the
    /// buffer was opened with (which may reach the file through a link the
    /// server's path doesn't), and its range is mapped onto the buffer as
    /// it is now, so text typed since doesn't move it off its name. If that
    /// version is below the buffer's edit log, the location's row is
    /// selected instead, as it is if the buffer has closed since: its text
    /// may not have been saved. Any other location is the file's, relative
    /// to the root, as loaded from disk. `None` without a root.
    pub(crate) fn target_of(&self, location: &Location, cx: &App) -> Option<(PathBuf, Reveal)> {
        let path = self.nav_state.root.as_ref()?.join(&location.rel);
        let Some((buffer, version)) = location.snapshot else {
            return Some((path, Reveal::bytes(location.range.clone())));
        };
        let row = Reveal {
            row: location.row,
            columns: 0..0,
            within_row: false,
        };
        let open = self.open_editors(cx).into_iter().find_map(|editor| {
            let model = editor.read(cx).model();
            let shared = model.buffer();
            (shared.id() == buffer).then(|| (model.path().map(Path::to_owned), shared.snapshot()))
        });
        let Some((opened, snapshot)) = open else {
            return Some((path, row));
        };
        let path = opened.unwrap_or(path);
        if version < snapshot.oldest_version() {
            return Some((path, row));
        }
        let range = &location.range;
        // Text typed at the name's start goes before it, and at its end
        // after it.
        let start = snapshot.map_offset(range.start, Bias::Right, version);
        let end = (snapshot.map_offset(range.end, Bias::Left, version)).max(start);
        Some((path, Reveal::bytes(start..end)))
    }

    pub(crate) fn run_nav_op(&mut self, op: NavOp, window: &mut Window, cx: &mut Context<Self>) {
        match op {
            NavOp::GotoDefinition => self.goto_definition(None, false, window, cx),
            NavOp::PeekDefinition => self.goto_definition(None, true, window, cx),
            NavOp::Related(kind) => self.goto_definition(Some(kind), false, window, cx),
            NavOp::WorkspaceSymbols => self.toggle_symbols(window, cx),
            NavOp::Back | NavOp::Forward => {
                // A go-to-definition still running would jump from elsewhere.
                self.nav_state.goto.next();
                // The caret's place is unknown until the file is read: going
                // now would lose it, so history is left as it is.
                if self.active_editor(cx).is_none() && self.pane().read(cx).place_pending() {
                    return;
                }
                let here = self.caret_place(cx);
                let history = &mut self.nav_state.history;
                let to = match op {
                    NavOp::Back => history.back(here),
                    _ => history.forward(here),
                };
                if let Some(Place { path, offset }) = to {
                    self.open_range(path, offset..offset, window, cx);
                }
                self.set_nav_buttons(self.nav_state.history.buttons(), cx);
            }
        }
    }

    /// Go to definition, or to the declaration, type definition or
    /// implementations `related` names; or, if `peek`, peek at the
    /// definitions in the editor ([`crate::peek`]).
    fn goto_definition(
        &mut self,
        related: Option<Related>,
        peek: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Not in a diff's side, which the pane doesn't show a peek over: as
        // go to definition there.
        let peek = peek && self.can_peek(cx);
        if self.nav_state.root.is_none() {
            let editor = self.active_editor(cx).cloned().filter(|_| peek);
            let status = text!("workbench-navigation-no-folder");
            return self.no_definitions(editor.as_ref(), status, window, cx);
        }
        let Some(request) = self.nav_request(cx) else {
            return;
        };
        // Where the request was made, which a jump goes back to.
        let editor = self.active_editor(cx).cloned();
        let from = self.caret_place(cx);
        // A peek opens under the caret only if it's still where it asked.
        let asked = editor
            .as_ref()
            .map(|editor| crate::peek::asked_at(editor, cx));
        let started = self.run_nav(
            |nav| &mut nav.goto,
            move |navigation, cancel| {
                let answer = match related {
                    Some(kind) => navigation.related(kind, &request, cancel),
                    None => navigation.definitions(&request, cancel),
                };
                (request.word(), answer)
            },
            window,
            cx,
            move |workbench, (word, answer), window, cx| {
                // Esc in the editor no longer drops it.
                workbench.nav_state.peek_escape = None;
                // Another tab shows now: the answer is for one left.
                let id = editor.as_ref().map(Entity::entity_id);
                if workbench.active_editor(cx).map(Entity::entity_id) != id {
                    return;
                }
                let peek = editor.filter(|_| peek);
                // Asked where the caret was, in the editor the pane shows.
                let moved = |editor: &Entity<_>| Some(crate::peek::asked_at(editor, cx)) != asked;
                if peek.as_ref().is_some_and(moved) || (peek.is_some() && !workbench.can_peek(cx)) {
                    return;
                }
                workbench.definitions_landed(from, related, word, answer, peek, window, cx);
            },
        );
        // Until it lands or another replaces it, Esc in the editor drops
        // it, whatever else Esc does there: closing the completion menu or
        // vim's insert session, say.
        self.nav_state.peek_escape = None;
        if started
            && peek
            && let Some(editor) = self.active_editor(cx)
        {
            let focus = editor.focus_handle(cx);
            let cancel = self.nav_state.goto.current().clone();
            let workbench = cx.entity().downgrade();
            // Ahead of the key's dispatch, which a bound Esc stops.
            let escape = cx.intercept_keystrokes(move |event, window, cx| {
                let pending = !cancel.load(Ordering::Relaxed);
                if event.keystroke.key == "escape" && pending && focus.is_focused(window) {
                    (workbench.update(cx, |workbench, _| workbench.nav_state.goto.next())).ok();
                }
            });
            self.nav_state.peek_escape = Some(escape);
        }
    }

    /// Jumps to an unambiguous definition, recording `from`, or lists the
    /// definitions, or says there are none. With a `peek` editor, any
    /// definitions open in a peek there instead.
    #[allow(clippy::too_many_arguments)]
    fn definitions_landed(
        &mut self,
        from: Option<Place>,
        related: Option<Related>,
        word: Option<String>,
        answer: Result<Answer<Definitions>, NavError>,
        peek: Option<Entity<fe_gpui::EditorView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The statuses' `kind` argument.
        let kind = match related {
            None => "definition",
            Some(Related::Declaration) => "declaration",
            Some(Related::TypeDefinition) => "type-definition",
            Some(Related::Implementation) => "implementation",
        };
        let answer = match answer {
            Ok(answer) => answer,
            Err(NavError::Cancelled) => return,
            // No provider has the answer: for the kinds search can't give,
            // that is, none found.
            Err(NavError::Unsupported) if related.is_some() => {
                let status = match word {
                    Some(word) => text!(
                        "workbench-navigation-not-found",
                        kind = kind,
                        word = word,
                        source = "exact",
                        provider = ""
                    ),
                    None => text!(
                        "workbench-navigation-no-symbol",
                        source = "exact",
                        provider = ""
                    ),
                };
                return self.no_definitions(peek.as_ref(), status, window, cx);
            }
            Err(error) => {
                let error = error.to_string();
                let status = text!("workbench-navigation-failed", kind = kind, error = error);
                return self.no_definitions(peek.as_ref(), status, window, cx);
            }
        };
        show_source(&self.status_bar().clone(), &answer.provider, cx);
        let (source, provider) = (source(answer.precision), answer.provider.clone());
        let Definitions {
            word,
            items,
            unambiguous,
            partial,
        } = answer.value;
        if let Some(editor) = &peek
            && !items.is_empty()
        {
            return self.open_peek(editor, items, window, cx);
        }
        if unambiguous && let Some(first) = items.first() {
            return self.jump_from(from, &first.location, window, cx);
        }
        let status = match (word, items.len()) {
            (None, _) => text!(
                "workbench-navigation-no-symbol",
                source = source,
                provider = provider
            ),
            (Some(word), 0) if partial => text!(
                "workbench-navigation-not-found-yet",
                kind = kind,
                word = word,
                source = source,
                provider = provider
            ),
            (Some(word), 0) => text!(
                "workbench-navigation-not-found",
                kind = kind,
                word = word,
                source = source,
                provider = provider
            ),
            (Some(word), count) => text!(
                "workbench-navigation-found",
                count = count,
                kind = kind,
                word = word,
                source = source,
                provider = provider
            ),
        };
        if peek.is_some() {
            return self.no_definitions(peek.as_ref(), status, window, cx);
        }
        self.show_definitions(items, status, window, cx);
    }

    /// Says `status`, there being no definitions: in the definitions
    /// picker, or for a peek in `peek`, in a toast, opening nothing, closing
    /// a peek open in that editor, and leaving focus where it is unless it
    /// was in that peek.
    fn no_definitions(
        &mut self,
        peek: Option<&Entity<fe_gpui::EditorView>>,
        status: Text,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::workbench::toast;
        match peek {
            Some(editor) => {
                self.close_peek_in(editor, window, cx);
                toast::show(cx, toast::Toast::info(status.resolve().to_string()));
            }
            None => self.show_definitions(Vec::new(), status, window, cx),
        }
    }

    /// Shows the definitions picker, focused, in place of any other palette.
    fn show_definitions(
        &mut self,
        items: Vec<fe_nav::Definition>,
        status: Text,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette != Some(Palette::Definitions) {
            self.dismiss_palette(window, cx);
        }
        let picker = self.nav_state.definitions.clone();
        let input = picker.focus_handle(cx);
        let previous = window.focused(cx).filter(|focused| *focused != input);
        picker.update(cx, |picker, cx| {
            picker.delegate.open(items, status, previous);
            picker.show(window, cx);
        });
        self.show_palette(Palette::Definitions, picker, window, cx);
    }

    /// Shows the workspace symbols picker, focused, in place of any other
    /// palette, or dismisses it.
    fn toggle_symbols(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picker = self.nav_state.symbols.clone();
        if self.palette == Some(Palette::Symbols) {
            return picker.update(cx, |picker, cx| picker.dismiss(window, cx));
        }
        self.dismiss_palette(window, cx);
        let ready = self.nav_state.start(cx);
        let previous = window.focused(cx);
        picker.update(cx, |picker, cx| {
            picker.delegate.opened(ready, previous);
            picker.show(window, cx);
        });
        self.show_palette(Palette::Symbols, picker, window, cx);
    }
}

/// What a nav command asks for: its `#[kiln::command]` method runs it
/// with [`Workbench::run_nav_op`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavOp {
    GotoDefinition,
    PeekDefinition,
    Related(Related),
    WorkspaceSymbols,
    Back,
    Forward,
}
