//! Spelling squiggles: a wavy underline in the Information tint under each
//! misspelled word of what an editor shows, behind `editor.spell_check`.
//!
//! Each editor that has been shown has a [`Squiggles`] provider, whose squiggles
//! follow edits until the next check replaces them. After each draw,
//! [`Workbench::sync_spell_squiggles`] looks at the editors that are showing:
//! once one holds still for [`REST`], the rows in view and [`MARGIN`] rows
//! either side that it hasn't checked are. Which of those rows' text to check
//! comes from the parse the editor has, on this thread; the [`Speller`] is asked
//! in the background. Rows stay checked until the buffer is edited, or the
//! settings or the dictionary change.
//!
//! A word is in one row, so a comment that goes on past those rows is checked
//! only as far as they go, however long it is.
//!
//! With [`NoSpeller`], which is the default, none of this runs: editors get no
//! provider, and nothing is spawned.

use std::{
    any::TypeId,
    cell::RefCell,
    collections::{HashMap, HashSet},
    io,
    ops::Range,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use fe_buffer::{BufferSnapshot, Version};
use fe_config::settings::SpellScope;
use fe_editor::decorations::{
    Decoration, DecorationKind, DecorationProvider, DecorationSet, ProviderId, Tint,
};
use fe_gpui::{EditorSettings, EditorView};
use fe_text::Text;
use gpui::{App, AppContext as _, Context, Entity, EntityId, Global, Task};

use crate::{
    Workbench,
    spell::{Dictionary, NoSpeller, Speller, check_text},
    spell_ranges::checked_spellable_ranges,
};

/// How long an editor holds still before it's checked.
pub(crate) const REST: Duration = Duration::from_millis(400);

/// How many rows either side of those in view are checked.
pub(crate) const MARGIN: u32 = 50;

/// How many more times rows are looked at when their highlights didn't finish.
/// Those cut short get more time, and a language that was missing gets compiled,
/// which can find another that's missing, but what's past a cap stays unfinished,
/// so this has to end.
const RETRIES: u32 = 8;

/// The settings that say what's checked.
#[derive(Clone, PartialEq)]
struct Wanted {
    on: bool,
    scopes: Vec<SpellScope>,
    language: String,
}

impl Wanted {
    fn new(settings: &EditorSettings) -> Self {
        Self {
            on: settings.spell_check,
            scopes: settings.spell_check_scopes.clone(),
            language: settings.spell_check_language.clone(),
        }
    }
}

/// The stock [`DecorationSet`], but for a word that's been deleted: the set
/// keeps it as an empty range, which would be painted as a short underline.
#[derive(Default)]
struct Squiggles(DecorationSet);

impl DecorationProvider for Squiggles {
    fn decorations(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        let from = out.len();
        self.0.decorations(snapshot, range, out);
        let mine = out.split_off(from);
        out.extend(mine.into_iter().filter(|it| !it.range.is_empty()));
    }

    fn refresh(&mut self, snapshot: &BufferSnapshot) {
        self.0.refresh(snapshot);
    }
}

/// An editor's squiggles and what's known of its rows.
struct Slot {
    editor: Entity<EditorView>,
    provider: ProviderId,
    squiggles: Rc<RefCell<Squiggles>>,
    /// The text `checked` is of.
    version: Version,
    /// The language `checked` was found with: `None` for plain text.
    language: Option<&'static str>,
    /// The rows checked, sorted and apart.
    checked: Vec<Range<u32>>,
    /// The wait, then the check.
    task: Option<Task<()>>,
    /// How many times in a row highlights didn't finish.
    retries: u32,
}

/// The words accepted since the app started, in any window, each once and only
/// ever added to. A window can open at any time, with a dictionary of its own that
/// hasn't heard of them, and replacing a dictionary mustn't lose them either.
#[derive(Default)]
struct SessionWords(Vec<String>);

impl Global for SessionWords {}

/// The workbench's spell checking.
pub(crate) struct Spelling {
    /// `None` for [`NoSpeller`].
    speller: Option<Arc<dyn Speller>>,
    dictionary: Arc<Dictionary>,
    /// How many of [`SessionWords`] `dictionary` has.
    session_words: usize,
    wanted: Wanted,
    slots: HashMap<EntityId, Slot>,
    /// Every provider given out since they last all went. A split copies its
    /// original's, which can have closed by the time the split is seen, so these
    /// can be on an editor with no slot.
    issued: Vec<ProviderId>,
    /// How many checks went to the background.
    #[cfg(test)]
    pub(crate) checks: usize,
}

impl Spelling {
    pub(crate) fn new(settings: &EditorSettings) -> Self {
        Self {
            speller: None,
            dictionary: Arc::default(),
            session_words: 0,
            wanted: Wanted::new(settings),
            slots: HashMap::new(),
            issued: Vec::new(),
            #[cfg(test)]
            checks: 0,
        }
    }

    /// Whether anything is to be checked.
    pub(crate) fn is_active(&self) -> bool {
        self.speller.is_some() && self.wanted.on
    }

    /// How many editors have a provider.
    #[cfg(test)]
    pub(crate) fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Whether editor `id` has a check waiting or running.
    #[cfg(test)]
    pub(crate) fn has_task(&self, id: EntityId) -> bool {
        (self.slots.get(&id)).is_some_and(|slot| slot.task.is_some())
    }
}

/// Some rows to check: their bytes, and the bytes in them that are words.
pub(crate) struct Piece {
    pub(crate) rows: Range<u32>,
    pub(crate) bytes: Range<usize>,
    pub(crate) spellable: Vec<Range<usize>>,
}

/// What a check found.
pub(crate) struct Landing {
    pub(crate) version: Version,
    pub(crate) pieces: Vec<Piece>,
    /// The misspelled words of all `pieces`, in order.
    pub(crate) found: Vec<Range<usize>>,
}

/// What to do about an editor's rows, as things are now.
enum Plan {
    /// Nothing can be said of them yet, and a draw comes when it can: the
    /// language is loading, or the parse is behind the text.
    NotReady,
    /// Highlighting didn't finish, with the languages it found missing.
    Unfinished(Vec<&'static str>),
    Check(Vec<Piece>),
}

impl Workbench {
    /// Spell checks with `speller` from now on. Off macOS, and until this is
    /// called, it's [`NoSpeller`], which costs nothing.
    pub fn set_speller<S: Speller + 'static>(&mut self, speller: S, cx: &mut Context<Self>) {
        let none = TypeId::of::<S>() == TypeId::of::<NoSpeller>();
        self.spelling.speller = (!none).then(|| Arc::new(speller) as Arc<dyn Speller>);
        self.drop_spell_squiggles(cx);
        cx.notify();
    }

    /// What's spell checked with, unless that's [`NoSpeller`].
    pub fn speller(&self) -> Option<Arc<dyn Speller>> {
        self.spelling.speller.clone()
    }

    /// `editor.spell_check_language`.
    pub fn spell_language(&self) -> &str {
        &self.spelling.wanted.language
    }

    /// The words the user has accepted.
    pub fn spell_dictionary(&self) -> &Dictionary {
        &self.spelling.dictionary
    }

    /// Replaces the words in the user's file, and checks again. Those accepted
    /// this session stay, as they do until the app quits. The
    /// workbench starts with an empty one that writes nothing, since reading the
    /// user's blocks on its file: the app reads it and hands it over here.
    pub fn set_spell_dictionary(&mut self, dictionary: Dictionary, cx: &mut Context<Self>) {
        self.spelling.dictionary = Arc::new(dictionary);
        self.spelling.session_words = 0;
        self.catch_up_session_words(cx);
        self.spell_dictionary_changed(cx);
    }

    /// Accepts `word` for good. Its squiggles go now; the dictionary's file is
    /// written in the background, as that blocks on the file and its lock. It
    /// counts for this session even if the write fails.
    pub fn add_spell_word(&mut self, word: &str, cx: &mut Context<Self>) {
        let line = word.to_owned();
        let writing = self.write_spell_word(word, cx);
        cx.background_spawn(async move {
            if let Err(error) = writing.await {
                log::warn!("can't add {line} to the dictionary: {error}");
            }
        })
        .detach();
    }

    /// [`Workbench::add_spell_word`], leaving what becomes of the write to the
    /// caller.
    pub(crate) fn write_spell_word(
        &mut self,
        word: &str,
        cx: &mut Context<Self>,
    ) -> Task<io::Result<()>> {
        // The session's own copy doesn't need what the write reads from the
        // file: only another window can have added that, and it's kept there.
        let mut writer = (*self.spelling.dictionary).clone();
        let line = word.to_owned();
        let writing = cx.background_spawn(async move { writer.add_word(&line) });
        self.ignore_spell_word(word, cx);
        writing
    }

    /// Accepts `word` until the app quits, writing nothing. Its squiggles go now.
    pub fn ignore_spell_word(&mut self, word: &str, cx: &mut Context<Self>) {
        let words = &mut cx.default_global::<SessionWords>().0;
        if !words.iter().any(|known| known == word) {
            words.push(word.to_owned());
        }
        Arc::make_mut(&mut self.spelling.dictionary).ignore_word(word);
        self.spell_dictionary_changed(cx);
    }

    /// Gives the dictionary the words accepted that it hasn't got yet: whether it
    /// had any to get.
    fn catch_up_session_words(&mut self, cx: &App) -> bool {
        let Some(SessionWords(words)) = cx.try_global::<SessionWords>() else {
            return false;
        };
        let new = &words[self.spelling.session_words..];
        if new.is_empty() {
            return false;
        }
        let dictionary = Arc::make_mut(&mut self.spelling.dictionary);
        for word in new {
            dictionary.ignore_word(word);
        }
        self.spelling.session_words = words.len();
        true
    }

    /// The bytes of `editor`'s misspelled words, in its model's text, in order.
    pub fn misspellings(&self, editor: &Entity<EditorView>, cx: &App) -> Vec<Range<usize>> {
        let Some(slot) = self.spelling.slots.get(&editor.entity_id()) else {
            return Vec::new();
        };
        let snapshot = editor.read(cx).model().snapshot();
        all_squiggles(&slot.squiggles.borrow(), snapshot)
            .into_iter()
            .map(|squiggle| squiggle.range)
            .collect()
    }

    /// Takes the squiggles over `range` of `editor`'s buffer from every open
    /// editor of that buffer, shown or not, ahead of the check the edit brings.
    pub(crate) fn forget_spell_squiggles_in(
        &mut self,
        editor: &Entity<EditorView>,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) {
        for slot in self.spelling.slots.values() {
            let shares = (slot.editor.read(cx).model()).shares_buffer_with(editor.read(cx).model());
            if !shares {
                continue;
            }
            slot.editor.update(cx, |view, cx| {
                view.model_mut().sync_display();
                let snapshot = view.model().snapshot();
                let mut squiggles = slot.squiggles.borrow_mut();
                let kept = (all_squiggles(&squiggles, snapshot).into_iter())
                    .filter(|it| it.range.end <= range.start || it.range.start >= range.end)
                    .collect();
                squiggles.0.apply(snapshot, snapshot.version(), kept);
                cx.notify();
            });
        }
    }

    /// Follows reloaded settings: if what's checked changed, every squiggle goes
    /// and the next draw starts again.
    pub(crate) fn reload_spell_squiggles(
        &mut self,
        settings: &EditorSettings,
        cx: &mut Context<Self>,
    ) {
        self.want_spelling(Wanted::new(settings), cx);
    }

    /// `editor.spell_check`, as last reloaded or set.
    pub(crate) fn spell_check_on(&self) -> bool {
        self.spelling.wanted.on
    }

    /// Follows `editor.spell_check` set to `on`, ahead of the reload to come.
    pub(crate) fn set_spell_check_on(&mut self, on: bool, cx: &mut Context<Self>) {
        let wanted = Wanted {
            on,
            ..self.spelling.wanted.clone()
        };
        self.want_spelling(wanted, cx);
    }

    fn want_spelling(&mut self, wanted: Wanted, cx: &mut Context<Self>) {
        if wanted != self.spelling.wanted {
            self.spelling.wanted = wanted;
            self.drop_spell_squiggles(cx);
            cx.notify();
        }
    }

    /// Takes every provider from its editor, and with it each check.
    fn drop_spell_squiggles(&mut self, cx: &mut Context<Self>) {
        self.adopt_open_editors(cx);
        self.spelling.issued.clear();
        for (_, slot) in self.spelling.slots.drain() {
            slot.editor.update(cx, |view, cx| {
                view.model_mut().remove_decoration_provider(slot.provider);
                cx.notify();
            });
        }
    }

    /// Takes the words now accepted from every open editor's squiggles, shown or
    /// not, and has what's shown checked again: a dictionary that was replaced
    /// can have lost words too, and a check under way used the old one.
    fn spell_dictionary_changed(&mut self, cx: &mut Context<Self>) {
        self.adopt_open_editors(cx);
        let dictionary = self.spelling.dictionary.clone();
        for slot in self.spelling.slots.values_mut() {
            slot.task = None;
            slot.retries = 0;
            slot.checked.clear();
            let snapshot = slot.editor.read(cx).model().snapshot().clone();
            let mut squiggles = slot.squiggles.borrow_mut();
            let kept = (all_squiggles(&squiggles, &snapshot).into_iter())
                .filter(|it| {
                    !dictionary.contains(&snapshot.text().slice_to_string(it.range.clone()))
                })
                .collect();
            squiggles.0.apply(&snapshot, snapshot.version(), kept);
            slot.editor.update(cx, |_, cx| cx.notify());
        }
        cx.notify();
    }

    /// Brings the squiggles of what's shown up to date: run after each draw, as
    /// that's what follows an edit, a scroll or a tab shown.
    pub(crate) fn sync_spell_squiggles(&mut self, cx: &mut Context<Self>) {
        if !self.spelling.is_active() {
            return;
        }
        // Before anything's checked in a window opened since they were accepted.
        if self.catch_up_session_words(cx) {
            self.spell_dictionary_changed(cx);
        }
        let open: HashSet<EntityId> = (self.open_editors(cx).iter())
            .map(Entity::entity_id)
            .collect();
        self.spelling.slots.retain(|id, _| open.contains(id));
        self.adopt_open_editors(cx);
        for editor in self.shown_editors(cx) {
            self.sync_spell_editor(editor, cx);
        }
    }

    /// The editor each pane is showing. One in a hidden tab has no viewport of
    /// its own to speak of, and its model isn't caught up with its buffer.
    fn shown_editors(&self, cx: &App) -> Vec<Entity<EditorView>> {
        (self.panes(cx).iter())
            .filter_map(|pane| pane.read(cx).active_editor().cloned())
            .collect()
    }

    /// Gives each open editor without a provider its own, in place of any it
    /// copied: hidden ones too, so that what a split copied can't outlive what
    /// it came from. With none given out and none wanted, there's nothing to do.
    fn adopt_open_editors(&mut self, cx: &mut Context<Self>) {
        if self.spelling.issued.is_empty() && !self.spelling.is_active() {
            return;
        }
        for editor in self.open_editors(cx) {
            if !self.spelling.slots.contains_key(&editor.entity_id()) {
                self.attach_spell_squiggles(editor, cx);
            }
        }
    }

    fn sync_spell_editor(&mut self, editor: Entity<EditorView>, cx: &mut Context<Self>) {
        let id = editor.entity_id();
        let view = editor.read(cx);
        let version = view.model().snapshot().version();
        let language = language_of(view);
        let rows = rows_to_check(view);
        let Some(slot) = self.spelling.slots.get_mut(&id) else {
            return;
        };
        if slot.language != language {
            // What it found was found as another language.
            slot.squiggles.borrow_mut().0.clear();
        }
        if slot.language != language || slot.version != version {
            // Starts the wait again, and drops a check of the old text.
            slot.language = language;
            slot.version = version;
            slot.checked.clear();
            slot.retries = 0;
            slot.task = Some(wait_then_check(id, Vec::new(), cx));
        } else if slot.task.is_none()
            && rows.is_some_and(|rows| !unchecked_rows(&rows, &slot.checked).is_empty())
        {
            slot.task = Some(wait_then_check(id, Vec::new(), cx));
        }
    }

    /// Gives `editor` its provider. A split starts with a copy of its original's
    /// providers, so those of ours go, for it to have one.
    fn attach_spell_squiggles(&mut self, editor: Entity<EditorView>, cx: &mut Context<Self>) {
        let copied = self.spelling.issued.clone();
        let squiggles = Rc::new(RefCell::new(Squiggles::default()));
        let provider = editor.update(cx, |view, _| {
            for id in copied {
                view.model_mut().remove_decoration_provider(id);
            }
            view.model_mut().add_decoration_provider(squiggles.clone())
        });
        self.spelling.issued.push(provider);
        let view = editor.read(cx);
        let slot = Slot {
            provider,
            squiggles,
            version: view.model().snapshot().version(),
            language: language_of(view),
            checked: Vec::new(),
            task: None,
            retries: 0,
            editor: editor.clone(),
        };
        self.spelling.slots.insert(editor.entity_id(), slot);
    }

    /// The wait is over: what to ask the speller, if anything. Rows with nothing
    /// to ask about land here and now.
    fn start_spell_check(
        &mut self,
        id: EntityId,
        cx: &mut Context<Self>,
    ) -> Option<impl Future<Output = Landing> + Send + use<>> {
        let shown = self.shown_editors(cx);
        let speller = self.spelling.speller.clone();
        let dictionary = self.spelling.dictionary.clone();
        let language = self.spelling.wanted.language.clone();
        let scopes = self.spelling.wanted.scopes.clone();
        let slot = self.spelling.slots.get_mut(&id)?;
        // This runs in `slot.task`, which goes only if there's nothing to await:
        // dropping it would cancel the check.
        let (Some(speller), true) = (speller, shown.contains(&slot.editor)) else {
            slot.task = None;
            return None;
        };
        let editor = slot.editor.clone();
        let view = editor.read(cx);
        let snapshot = view.model().snapshot().clone();
        match plan(view, &scopes, &slot.checked) {
            Plan::NotReady => {
                slot.task = None;
                None
            }
            Plan::Unfinished(missing) if slot.retries < RETRIES => {
                slot.retries += 1;
                slot.task = Some(wait_then_check(id, missing, cx));
                None
            }
            // The rows have no words anyone can find.
            Plan::Unfinished(_) => {
                let Some(window) = rows_to_check(view) else {
                    slot.task = None;
                    return None;
                };
                let pieces = (unchecked_rows(&window, &slot.checked).into_iter())
                    .map(|rows| Piece {
                        bytes: row_bytes(snapshot.text(), &rows),
                        rows,
                        spellable: Vec::new(),
                    })
                    .collect();
                let landing = Landing {
                    version: snapshot.version(),
                    pieces,
                    found: Vec::new(),
                };
                self.spell_check_landed(id, landing, cx);
                None
            }
            Plan::Check(pieces) if pieces.iter().all(|piece| piece.spellable.is_empty()) => {
                let landing = Landing {
                    version: snapshot.version(),
                    pieces,
                    found: Vec::new(),
                };
                self.spell_check_landed(id, landing, cx);
                None
            }
            Plan::Check(pieces) => {
                #[cfg(test)]
                {
                    self.spelling.checks += 1;
                }
                Some(async move {
                    let found = misspelled(&*speller, &dictionary, &snapshot, &pieces, &language);
                    Landing {
                        version: snapshot.version(),
                        pieces,
                        found,
                    }
                })
            }
        }
    }

    /// Shows what a check found, unless the text has moved on since.
    pub(crate) fn spell_check_landed(
        &mut self,
        id: EntityId,
        landing: Landing,
        cx: &mut Context<Self>,
    ) {
        let Some(slot) = self.spelling.slots.get_mut(&id) else {
            return;
        };
        slot.task = None;
        let editor = slot.editor.clone();
        let model = editor.read(cx).model();
        let snapshot = model.snapshot().clone();
        // The buffer's version: the model's snapshot trails it until a frame.
        if model.buffer().version() == landing.version {
            let mut squiggles = slot.squiggles.borrow_mut();
            for piece in landing.pieces {
                let items: Vec<_> = (landing.found.iter())
                    .filter(|word| piece.bytes.contains(&word.start))
                    .map(|word| squiggle(word.clone()))
                    .collect();
                let bytes = piece.bytes;
                if !(squiggles.0).splice(&snapshot, landing.version, bytes.clone(), items.clone()) {
                    // The anchors it has are too old to keep, so what it has is
                    // to be found again.
                    squiggles.0.clear();
                    slot.checked.clear();
                    (squiggles.0).splice(&snapshot, landing.version, bytes, items);
                }
                add_rows(&mut slot.checked, piece.rows);
            }
            slot.retries = 0;
            drop(squiggles);
            editor.update(cx, |_, cx| cx.notify());
        }
        // It may have been scrolled, or edited with no draw yet.
        self.sync_spell_squiggles(cx);
    }
}

/// What happens to editor `id` once it has held still: the check, off this
/// thread, of its rows then. A check can find `missing` languages where nothing
/// is drawn, which leaves none to compile them, so that's done first.
fn wait_then_check(
    id: EntityId,
    missing: Vec<&'static str>,
    cx: &mut Context<Workbench>,
) -> Task<()> {
    cx.spawn(async move |workbench, cx| {
        if !missing.is_empty() {
            cx.background_spawn(async move {
                for name in missing {
                    fe_syntax::language_by_name(name);
                }
            })
            .await;
        }
        cx.background_executor().timer(REST).await;
        let Ok(Some(check)) =
            workbench.update(cx, |workbench, cx| workbench.start_spell_check(id, cx))
        else {
            return;
        };
        let landing = cx.background_spawn(check).await;
        workbench
            .update(cx, |workbench, cx| {
                workbench.spell_check_landed(id, landing, cx)
            })
            .ok();
    })
}

/// A misspelled word's decoration.
fn squiggle(range: Range<usize>) -> Decoration {
    Decoration::new(
        range,
        DecorationKind::Underline { wavy: true },
        Tint::Information,
    )
}

/// Everything in `squiggles`, in `snapshot`.
fn all_squiggles(squiggles: &Squiggles, snapshot: &BufferSnapshot) -> Vec<Decoration> {
    let mut all = Vec::new();
    squiggles.decorations(snapshot, 0..snapshot.len(), &mut all);
    all
}

/// The language `view` checks its text as: `None` for plain text.
fn language_of(view: &EditorView) -> Option<&'static str> {
    view.syntax().map(|syntax| syntax.language().name())
}

/// The rows `view`'s last frame laid out, and [`MARGIN`] rows either side,
/// sorted and apart: none in a fold, however many it hides, and `None` before a
/// frame shows any. `viewport_rows` counts what fits in the view, which blocks can
/// take up, and a wrapped row or a fold keeps from being as many buffer rows. So
/// that only bounds where to look: from the first display row the view can start
/// on, the view's start being partway down a wrapped row, past where it can end.
fn rows_to_check(view: &EditorView) -> Option<Vec<Range<u32>>> {
    let (top, fits) = view.viewport_rows()?;
    let model = view.model();
    let display = model.display();
    let rows = model.snapshot().text().row_count();
    let line = view.top_buffer_line()?.max(0.);
    let row = (line.floor() as u32).min(top);
    let start = display.row_for_buffer_row(row);
    let next = match row + 1 < rows {
        true => display.row_for_buffer_row(row + 1),
        false => display.row_count(),
    };
    // Not from the row's start, which a very long row puts far up. The top is a
    // fraction, so a row to spare each way, and a row more for one partly in view.
    let down = (line.fract() * next.saturating_sub(start) as f32) as u32;
    let start = start + down.saturating_sub(1);
    let end = (start.saturating_add(fits).saturating_add(3)).min(display.row_count());
    let mut laid_out: Vec<Range<u32>> = Vec::new();
    for at in start..end {
        let shown = display.row(at).buffer_row;
        if view.shows_buffer_row(shown) != Some(true) {
            continue;
        }
        match laid_out.last_mut() {
            Some(last) if shown <= last.end => last.end = last.end.max(shown + 1),
            _ => laid_out.push(shown..shown + 1),
        }
    }
    let first = laid_out.first_mut()?;
    first.start = first.start.saturating_sub(MARGIN);
    let last = laid_out.last_mut()?;
    last.end = last.end.saturating_add(MARGIN).min(rows);
    Some(laid_out)
}

/// The rows of `wanted` not in `checked`.
fn unchecked_rows(wanted: &[Range<u32>], checked: &[Range<u32>]) -> Vec<Range<u32>> {
    (wanted.iter())
        .flat_map(|rows| missing_rows(rows, checked))
        .collect()
}

/// What to do about `view`'s rows in view that aren't in `checked`.
fn plan(view: &EditorView, scopes: &[SpellScope], checked: &[Range<u32>]) -> Plan {
    let snapshot = view.model().snapshot();
    let syntax = view.syntax();
    // Plain text only until its language loads: its code isn't prose.
    let behind = view.model().buffer().version() != snapshot.version()
        || syntax.is_some_and(|it| it.version() != snapshot.version() || it.needs_parse());
    if view.is_loading_language() || behind {
        return Plan::NotReady;
    }
    let Some(window) = rows_to_check(view) else {
        return Plan::NotReady;
    };
    let text = snapshot.text();
    let mut pieces = Vec::new();
    for rows in unchecked_rows(&window, checked) {
        let bytes = row_bytes(text, &rows);
        let found = checked_spellable_ranges(syntax, text, scopes, rows.clone());
        if !found.complete {
            let missing = syntax.map(|it| it.take_missing_languages());
            return Plan::Unfinished(missing.unwrap_or_default());
        }
        // Whole tokens may go on past the rows.
        let spellable = (found.ranges.into_iter())
            .map(|range| range.start.max(bytes.start)..range.end.min(bytes.end))
            .filter(|range| !range.is_empty())
            .collect();
        pieces.push(Piece {
            rows,
            bytes,
            spellable,
        });
    }
    Plan::Check(pieces)
}

/// The misspelled words of `pieces`, in order. Off the main thread.
fn misspelled(
    speller: &dyn Speller,
    dictionary: &Dictionary,
    snapshot: &BufferSnapshot,
    pieces: &[Piece],
    language: &str,
) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    for range in pieces.iter().flat_map(|piece| &piece.spellable) {
        let source = snapshot.text().slice_to_string(range.clone());
        found.extend(
            (check_text(speller, dictionary, &source, language).into_iter())
                .map(|word| range.start + word.range.start..range.start + word.range.end),
        );
    }
    found
}

/// The bytes of `rows`, through the line break of the last.
fn row_bytes(text: &Text, rows: &Range<u32>) -> Range<usize> {
    let end = match rows.end < text.row_count() {
        true => text.row_start(rows.end),
        false => text.len(),
    };
    text.row_start(rows.start)..end
}

/// The parts of `window` not in `checked`, which is sorted and apart.
pub(crate) fn missing_rows(window: &Range<u32>, checked: &[Range<u32>]) -> Vec<Range<u32>> {
    let mut missing = Vec::new();
    let mut from = window.start;
    for done in checked {
        if done.end <= from {
            continue;
        }
        if done.start >= window.end {
            break;
        }
        if from < done.start {
            missing.push(from..done.start);
        }
        from = done.end;
    }
    if from < window.end {
        missing.push(from..window.end);
    }
    missing
}

/// Adds `rows` to `checked`, keeping it sorted and apart.
pub(crate) fn add_rows(checked: &mut Vec<Range<u32>>, rows: Range<u32>) {
    if rows.is_empty() {
        return;
    }
    checked.push(rows);
    checked.sort_by_key(|rows| rows.start);
    let mut merged: Vec<Range<u32>> = Vec::with_capacity(checked.len());
    for rows in checked.drain(..) {
        match merged.last_mut() {
            Some(last) if rows.start <= last.end => last.end = last.end.max(rows.end),
            _ => merged.push(rows),
        }
    }
    *checked = merged;
}
