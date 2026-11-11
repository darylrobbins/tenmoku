//! [`DiffView`]: two texts side by side in one kiln-shell pane item, the
//! left (the old text: a file in `HEAD` or the index, say) and the right
//! (the new: usually the file's live buffer). Each side is an
//! [`EditorView`] with a header naming it. Rows line up across the two,
//! the shorter side of each hunk padded with filler blocks
//! ([`TextDiff::fillers`]), hatched; the left side's removed and modified
//! rows are tinted removed, the right side's added and modified rows added,
//! and their changed words more strongly ([`DiffTints`]): within a modified
//! row pair the words [`InlineDiff::changed_spans`] finds, and the whole
//! text of a row with no partner, in the theme's `diff_*` tokens. Each
//! hunk has a header row above it on both sides naming the symbols around
//! it and how many rows it removes and adds, `impl MinimapTile › render · 4
//! changes` ([`DiffView::hunk_header`]), and each changed row a `−` (left)
//! or `+` (right) at the gutter's left edge ([`Glyph`]), so colour is never
//! the only cue. Scrolling either
//! side scrolls the other to the same screen row, and `diff.next_change` /
//! `diff.previous_change` move between hunks, wrapping at the ends.
//! `diff.toggle_inline` shows the right side alone instead, the removed
//! rows as blocks above their place ([`inline`]). Unchanged rows away from
//! the hunks are hidden on both sides behind `36 hidden lines` rows
//! ([`fold`]), until `diff.toggle_folding` shows them.
//!
//! The diff is redone when either side's buffer moves on, at most once a
//! frame (on render), off the main thread when a side is over
//! [`ASYNC_BYTES`]; the previous fillers and tints stay until it lands.
//! Each side's model gets row changes of its own
//! ([`EditorModel::own_row_changes`]), against the other side's text,
//! supplied from that diff: its change bars and vim's `]c` / `[c` follow
//! the diff, the file's other editors keep comparing against git, and no
//! paint diffs on the main thread.

use std::{
    cell::RefCell,
    hash::{DefaultHasher, Hash as _, Hasher as _},
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use fe_buffer::{Bias, BufferSnapshot, Text, Version};
use fe_command::{
    Actor, CommandId, CommandTarget, ContextFrame, ContextPredicate, ContextStack, Dispatched,
    Effect, Registry, Value,
};
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, Tint, overlaps},
    inline_diff::InlineDiff,
    row_changes::{RowChange, RowChangeKind},
    text_diff::{HunkKind, Side, TextDiff},
};

use fe_ui::{Icon, IconButton, Tooltip};
use gpui::{
    AnyElement, AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyContext, ParentElement as _, PromptLevel, Render,
    SharedString, Styled as _, Subscription, Task, Window, div, px,
};
use kiln_l10n::tr;
use kiln_shell::{CloseAnswer, PaneItem};

#[path = "diff_fold.rs"]
mod fold;
mod inline;

pub use fold::{CONTEXT_ROWS, EXPAND_ROWS, Expand, Glyph, HiddenBlock, crumb};

use crate::{
    EditorView,
    commands::{EditorViewEvent, HostBlock, HostBlockKind, RunCommand},
    pane_item::{discard_buttons, restore_style},
};

/// Key context active while either side of a diff view is focused.
pub const KEY_CONTEXT: &str = "DiffView";

/// Moves the focused side's caret to the next hunk, wrapping to the first.
pub const NEXT_CHANGE: &str = "diff.next_change";

/// Moves the focused side's caret to the previous hunk, wrapping to the
/// last.
pub const PREVIOUS_CHANGE: &str = "diff.previous_change";

/// Switches between side by side and inline.
pub const TOGGLE_INLINE: &str = "diff.toggle_inline";

/// Hides unchanged rows away from the hunks, or shows them all.
pub const TOGGLE_FOLDING: &str = "diff.toggle_folding";

/// A side over this many bytes is diffed off the main thread.
pub const ASYNC_BYTES: usize = 1024 * 1024;

/// A side over this many bytes also has its changed words found off the
/// main thread: they're found for every paired row, offscreen ones
/// included, after every edit, and noisy text of this size costs tens of
/// milliseconds there.
const WORDS_ASYNC_BYTES: usize = 64 * 1024;

/// What navigation says when it wraps past the last hunk: `Wrapped to
/// first change`.
pub fn wrapped_to_first() -> SharedString {
    tr!("gpui-diff-wrapped-to-first")
}

/// What navigation says when it wraps past the first hunk: `Wrapped to
/// last change`.
pub fn wrapped_to_last() -> SharedString {
    tr!("gpui-diff-wrapped-to-last")
}

/// What navigation says when the texts are the same: `No changes`.
pub fn no_changes() -> SharedString {
    tr!("gpui-diff-no-changes")
}

/// What the header and navigation say when a side is over
/// [`text_diff::MAX_DIFF_BYTES`](fe_editor::text_diff::MAX_DIFF_BYTES), so
/// nothing was compared: `Too large to compare`.
pub fn too_large() -> SharedString {
    tr!("gpui-diff-too-large")
}

/// What navigation last said: see [`DiffView::message`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Said {
    WrappedToFirst,
    WrappedToLast,
    NoChanges,
    TooLarge,
}

impl Said {
    /// In the active locale.
    fn text(self) -> SharedString {
        match self {
            Said::WrappedToFirst => wrapped_to_first(),
            Said::WrappedToLast => wrapped_to_last(),
            Said::NoChanges => no_changes(),
            Said::TooLarge => too_large(),
        }
    }
}

/// The en-US messages, which the tests compare with.
#[cfg(test)]
pub(crate) const WRAPPED_TO_FIRST: &str = "Wrapped to first change";
#[cfg(test)]
pub(crate) const WRAPPED_TO_LAST: &str = "Wrapped to last change";
#[cfg(test)]
pub(crate) const NO_CHANGES: &str = "No changes";
#[cfg(test)]
pub(crate) const TOO_LARGE: &str = "Too large to compare";

/// How often the view looks for edits made through a model it doesn't
/// show, such as the file's editor in another window, which redraw
/// neither it nor its sides.
const POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// The answer to [`discard_buttons`] that closes.
const DISCARD: usize = 1;

/// The [`CommandTarget`] of the diff view's commands, which its
/// [`RunCommand`] handler runs: registered so keymaps and the palette know
/// them.
pub(crate) struct DiffCommands;

impl CommandTarget for DiffCommands {
    type Tx = ();
    type Target<'a> = ();
}

/// [`NEXT_CHANGE`], [`PREVIOUS_CHANGE`], [`TOGGLE_INLINE`] and
/// [`TOGGLE_FOLDING`], in the [`KEY_CONTEXT`] context. Their handlers do
/// nothing: the view runs them.
pub(crate) fn registry() -> Registry<DiffCommands> {
    let commands = [NEXT_CHANGE, PREVIOUS_CHANGE, TOGGLE_INLINE, TOGGLE_FOLDING];
    let mut registry = Registry::new();
    for id in commands {
        let title = crate::l10n::command_title(id);
        registry
            .register(id, &title, |_: &mut (), _: &Actor| {
                Ok(Dispatched::new(Effect::Nothing))
            })
            .expect("diff command IDs are valid and distinct")
            .context(ContextPredicate::identifier(KEY_CONTEXT));
    }
    registry
}

/// One side of a [`DiffView`]: its header's title (`HEAD`, `Index`,
/// `Working Tree`, a path) and the model it shows.
pub struct DiffSide {
    pub title: SharedString,
    pub model: EditorModel,
    /// Whether the side refuses edits. The left side always does.
    pub read_only: bool,
}

/// Sets the language of each of `views` with no path to go by, going by the
/// file's name, `named`, and failing that the right side's, over what its first
/// lines alone said when the view was made. It's detected, so not chosen. One
/// chosen by hand, or told where to look
/// ([`EditorView::detect_language_from`]), is left alone, and one that has the
/// language already, or has it loading, isn't reset.
fn detect_side_languages<T>(views: &[Entity<EditorView>; 2], named: &Path, cx: &mut Context<T>) {
    let language = |view: &Entity<EditorView>, cx: &App| {
        let view = view.read(cx);
        let text = view.model().text();
        let start = text
            .slice_to_string(0..text.clip_offset(crate::view::DETECT_BYTES, fe_buffer::Bias::Left));
        fe_syntax::detect_language_name(Some(view.language_path().unwrap_or(named)), &start)
    };
    let right_language = language(&views[1], cx);
    for side in views {
        let view = side.read(cx);
        if view.language_path().is_some() || view.is_language_chosen() {
            continue;
        }
        let found = language(side, cx).or(right_language);
        if view.chosen_language == found {
            continue;
        }
        side.update(cx, |view, cx| {
            view.set_language(found, cx);
            view.language_chosen = false;
        });
    }
}

impl DiffSide {
    /// An editable side.
    pub fn new(title: impl Into<SharedString>, model: EditorModel) -> Self {
        Self {
            title: title.into(),
            model,
            read_only: false,
        }
    }

    /// A side that refuses edits: a model over a fixed text, such as a
    /// commit's.
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }
}

/// Two texts compared side by side: see the [module docs](self).
pub struct DiffView {
    file_name: SharedString,
    /// The file compared, which tells its tab from others with the same
    /// title ([`DiffView::with_compared_path`]).
    compared_path: Option<PathBuf>,
    titles: [SharedString; 2],
    views: [Entity<EditorView>; 2],
    /// Each side's tints, registered on its model.
    tints: [Rc<RefCell<DiffTints>>; 2],
    /// The diff the fillers and tints show.
    diff: TextDiff,
    /// The buffer versions last diffed, or being diffed.
    diffed: Option<[Version; 2]>,
    /// A diff running off the main thread.
    diffing: Option<Task<()>>,
    /// What navigation last said about the diff shown, in the header:
    /// kept as what it was, so the header says it in the locale it's drawn
    /// in.
    message: Option<Said>,
    /// [`DiffView::message`]'s text: `message` resolved as it was set, and
    /// again each frame, so it's in the locale drawn in.
    message_text: Option<SharedString>,
    /// Whether the right side's buffer was dirty when last looked at: a
    /// save elsewhere changes it without changing the text.
    dirty: bool,
    /// The hunk navigation last went to, and where it put each side's
    /// caret: while the focused side's caret is still there, the next move
    /// counts from that hunk, so one at the end of a side, whose start is
    /// past the last row, isn't taken for the row the caret's on. `None`
    /// for a side it didn't move: the left, inline.
    visited: Option<(usize, [Option<usize>; 2])>,
    /// Whether only the right side shows, the removed rows inline.
    inline: bool,
    /// Whether the mode was chosen, by the user or the host, so the window's
    /// width doesn't pick it.
    chosen: bool,
    /// Whether the first frame has been drawn.
    sized: bool,
    /// The texts last diffed.
    texts: [Arc<str>; 2],
    /// How many rows each has.
    rows: [u32; 2],
    /// Each side's changed words in the diff shown.
    words: [Arc<Words>; 2],
    /// Whether unchanged rows away from the hunks are hidden.
    folding: bool,
    /// The left rows shown of those folding would hide, sorted and apart.
    revealed: Vec<Range<u32>>,
    /// The blocks hidden.
    hidden: Vec<HiddenBlock>,
    /// Each side's folds as last set, as `(start, end)`, and the buffer
    /// version they were set at.
    folded: [Option<fold::Folded>; 2],
    /// Each side's outline, for the labels of the blocks hidden, and the
    /// language and syntax version it's of.
    outlines: [Option<(fold::OutlineKey, Arc<fe_syntax::Outline>)>; 2],
    /// An outline being found off the main thread, and what it's of.
    outlining: [Option<(fold::OutlineKey, Task<()>)>; 2],
    /// Each side's carets' heads when last looked at, which tell a caret
    /// moved.
    heads: [Vec<usize>; 2],
    /// Whether each side's initial caret has been touched: moved from where
    /// the view opened it, kept across refolds.
    touched: [bool; 2],
    /// A right row revealed, and the rows the view shows to centre it in,
    /// shown and scrolled to again as each diff lands until one of the
    /// texts as they are has, or a side is scrolled.
    revealing: Option<(u32, Option<u32>)>,
    _subscriptions: Vec<Subscription>,
    /// Looks for edits from elsewhere every [`POLL`].
    _poll: Task<()>,
    /// The popover of the sides' glyph columns, none, which tells their
    /// annotations from a host's ([`DiffView::show_glyph_column`]).
    #[allow(clippy::type_complexity)]
    glyph_column: Rc<dyn Fn(usize, u32) -> Option<crate::HoverContent>>,
    /// How many times each side's scroll top was set from the other's.
    #[cfg(test)]
    pub(crate) scroll_sets: [usize; 2],
}

impl DiffView {
    /// Compares `left` with `right`, in the theme and settings of
    /// [`crate::set_restore_style`]. The left side is made read-only. Each
    /// side's model gets row changes of its own, against the other side
    /// (see the [module docs](self)); `right.model` may be a split of the
    /// file's editor, whose row changes stay as they are.
    pub fn new(
        file_name: impl Into<SharedString>,
        left: DiffSide,
        right: DiffSide,
        cx: &mut Context<Self>,
    ) -> Self {
        let (theme, settings) = restore_style(cx);
        let tints = [Tint::DiffRemoved, Tint::DiffAdded]
            .map(|tint| Rc::new(RefCell::new(DiffTints::new(tint))));
        let titles = [left.title, right.title];
        let mut view = |mut model: EditorModel, read_only: bool, tints: &Rc<RefCell<DiffTints>>| {
            model.own_row_changes();
            // Absolute numbers, whatever the editor split from shows.
            model.fix_absolute_line_numbers();
            model.add_decoration_provider(tints.clone());
            let theme = theme.clone();
            let settings = settings.clone();
            cx.new(move |cx| {
                let mut view = EditorView::new(model, theme, cx);
                view.apply_settings(&settings, cx);
                view.set_read_only(read_only, cx);
                view
            })
        };
        let views = [
            view(left.model, true, &tints[0]),
            view(right.model, right.read_only, &tints[1]),
        ];
        // A git side has no path, and its text lands after the view opens, so
        // `EditorView::new` found it no language; the file's name does.
        let file_name = file_name.into();
        detect_side_languages(&views, Path::new(file_name.as_ref()), cx);
        let mut subscriptions = Vec::new();
        for (from, to) in [(0, 1), (1, 0)] {
            // A side's edits, carets and scrolls may change the diff.
            subscriptions.push(cx.observe(&views[from], |_, _, cx| cx.notify()));
            subscriptions.push(cx.subscribe(
                &views[from],
                move |this: &mut Self, _, event: &EditorViewEvent, cx| match *event {
                    EditorViewEvent::Scrolled { top } => {
                        // Scrolled away from a row being revealed.
                        this.revealing = None;
                        if !this.inline {
                            this.follow(to, top, cx);
                        }
                    }
                    // A side's change bar is the host's to act on; it doesn't move the other side.
                    EditorViewEvent::ChangeBarClicked { .. } => {}
                    // As is a jump within a side.
                    EditorViewEvent::Jumped { .. } => {}
                },
            ));
        }
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                if this.update(cx, |this, cx| this.sync_buffers(cx)).is_err() {
                    break;
                }
            }
        });
        let mut this = Self {
            file_name,
            compared_path: None,
            titles,
            views,
            tints,
            diff: TextDiff::default(),
            diffed: None,
            diffing: None,
            message: None,
            message_text: None,
            dirty: false,
            visited: None,
            inline: false,
            chosen: false,
            sized: false,
            texts: [Arc::from(""), Arc::from("")],
            rows: [1, 1],
            words: Default::default(),
            folding: true,
            revealed: Vec::new(),
            hidden: Vec::new(),
            folded: [None, None],
            outlines: [None, None],
            outlining: [None, None],
            heads: Default::default(),
            touched: [false; 2],
            revealing: None,
            _subscriptions: subscriptions,
            _poll: poll,
            glyph_column: Rc::new(|_, _| None),
            #[cfg(test)]
            scroll_sets: [0; 2],
        };
        this.sync_buffers(cx);
        this
    }

    /// Names `path` as the file compared, for the tab's
    /// [`PaneItem::tab_path`], when neither side's model has it: a commit's
    /// diff, or `HEAD` against the index. It isn't the view's
    /// [`PaneItem::path`], so finding a file's tab never finds this one.
    pub fn with_compared_path(mut self, path: PathBuf) -> Self {
        self.compared_path = Some(path);
        self
    }

    /// Detects again the language of each side with no path to detect it from,
    /// unless one was picked for it: for when a language has become known since
    /// the view opened.
    pub fn detect_languages_again(&mut self, cx: &mut Context<Self>) {
        let views = self.views.clone();
        detect_side_languages(&views, Path::new(self.file_name.as_ref()), cx);
    }

    pub fn left(&self) -> &Entity<EditorView> {
        &self.views[0]
    }

    pub fn right(&self) -> &Entity<EditorView> {
        &self.views[1]
    }

    /// The right side's model: usually a split of the file's editor.
    pub(crate) fn right_model<'a>(&self, cx: &'a App) -> &'a EditorModel {
        self.views[1].read(cx).model()
    }

    /// The diff the fillers and tints show now.
    pub fn diff(&self) -> &TextDiff {
        &self.diff
    }

    /// Whether a diff is running off the main thread, the last one's
    /// fillers and tints showing meanwhile. For tests outside the crate.
    #[doc(hidden)]
    pub fn diff_pending(&self) -> bool {
        self.diffing.is_some()
    }

    /// What navigation last said: that it wrapped, found no changes, or
    /// that the texts are too large to compare.
    pub fn message(&self) -> Option<&SharedString> {
        self.message_text.as_ref()
    }

    /// Notes what navigation said, or that it said nothing.
    fn say(&mut self, said: Option<Said>) {
        self.message = said;
        self.message_text = said.map(Said::text);
    }

    /// Scrolls side `to` to screen row `top`, where the other side's first
    /// row in view is: fillers line the rows up, so it's the same place.
    fn follow(&mut self, to: usize, top: f32, cx: &mut Context<Self>) {
        #[cfg(test)]
        {
            self.scroll_sets[to] += 1;
        }
        self.views[to].update(cx, |view, cx| view.set_scroll_top(top, cx));
    }

    /// Catches up with the buffers: notes a change in the right one's
    /// dirtiness, for the tab, and diffs again if either's text changed.
    fn sync_buffers(&mut self, cx: &mut Context<Self>) {
        let dirty = self.views[1].read(cx).model().is_dirty();
        if dirty != self.dirty {
            self.dirty = dirty;
            cx.notify();
        }
        self.sync_diff(cx);
    }

    /// Diffs the sides again if either buffer moved on since the last diff:
    /// at once when both are small, otherwise off the main thread, the
    /// last diff's fillers and tints staying until it lands. An edit
    /// redraws every window, so another editor of the buffer, which nothing
    /// else tells, shows it.
    fn sync_diff(&mut self, cx: &mut Context<Self>) {
        let versions = (self.views.each_ref()).map(|view| view.read(cx).model().buffer().version());
        if self.diffed == Some(versions) {
            return;
        }
        if self.diffed.is_some() {
            cx.defer(|cx| cx.refresh_windows());
        }
        self.diffed = Some(versions);
        let [left, right] =
            (self.views.each_ref()).map(|view| view.read(cx).model().buffer().text().clone());
        if !Diffed::is_large(&left, &right) {
            self.diffing = None;
            let diffed = Diffed::new(&left, &right, versions);
            self.land(diffed, cx);
            return;
        }
        let diffing = cx.background_spawn(async move { Diffed::new(&left, &right, versions) });
        self.diffing = Some(cx.spawn(async move |this, cx| {
            let diffed = diffing.await;
            this.update(cx, |this, cx| {
                this.diffing = None;
                this.land(diffed, cx);
            })
            .ok();
        }));
    }

    /// Shows `diffed`: each side's fillers and tints, and its row changes
    /// against the other side's text, for the version of its text diffed.
    fn land(&mut self, diffed: Diffed, cx: &mut Context<Self>) {
        let Diffed {
            diff,
            texts,
            rows,
            words,
            versions,
        } = diffed;
        self.diff = diff;
        self.texts = texts;
        self.rows = rows;
        self.words = words.map(Arc::new);
        for (ix, side) in [(0, Side::Left), (1, Side::Right)] {
            let mut tints = self.tints[ix].borrow_mut();
            tints.rows = tinted_rows(&self.diff, side);
            tints.words = self.words[ix].clone();
            drop(tints);
            let base = self.texts[1 - ix].clone();
            let changes = row_changes(&self.diff, side);
            self.views[ix].update(cx, |view, cx| {
                view.model_mut()
                    .supply_row_changes(base, versions[ix], changes);
                // The tints changed.
                cx.notify();
            });
        }
        self.refold(cx);
        self.show_glyph_column(cx);
        self.go_on_revealing(cx);
        // What navigation said was about the last diff.
        self.say(None);
        self.visited = None;
        cx.notify();
    }

    /// Places each side's blocks.
    fn show_blocks(&mut self, cx: &mut Context<Self>) {
        for ix in 0..2 {
            let blocks = self.blocks_of(ix);
            self.views[ix].update(cx, |view, cx| view.set_blocks(blocks, cx));
        }
    }

    /// Side `ix`'s blocks: each hunk's header row, then fillers side by
    /// side, inline the removed rows above the right side's; and the label
    /// rows of the blocks hidden.
    fn blocks_of(&self, ix: usize) -> Vec<HostBlock> {
        let side = [Side::Left, Side::Right][ix];
        if self.inline && ix == 0 {
            return Vec::new();
        }
        let mut blocks = self.header_blocks(ix);
        match self.inline {
            true => blocks.extend(inline::removed_blocks(
                &self.diff,
                &self.texts[0],
                &self.words[0],
            )),
            false => blocks.extend((self.diff.fillers(side).into_iter().enumerate()).map(
                |(id, (above, rows))| HostBlock {
                    above,
                    rows,
                    kind: HostBlockKind::Filler,
                    id: id as u64,
                },
            )),
        }
        blocks.extend(self.label_blocks(ix));
        blocks
    }

    /// Moves the focused side's caret to the start of the next hunk
    /// (`forward`) or the previous one, wrapping at the ends; the other
    /// side's caret goes to the hunk's start there, and its scroll follows.
    fn go_to_change(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let focused = match !self.inline && self.views[0].focus_handle(cx).is_focused(window) {
            true => 0,
            false => 1,
        };
        let side = [Side::Left, Side::Right][focused];
        let model = self.views[focused].read(cx).model();
        let (row, head) = (caret_row(model), model.primary_selection().head());
        let count = self.diff.hunks().len();
        if count == 0 {
            let message = match self.diff.too_large() {
                true => Said::TooLarge,
                false => Said::NoChanges,
            };
            self.say(Some(message));
            cx.notify();
            return;
        }
        let found = match (self.visited, forward) {
            (Some((index, carets)), true) if carets[focused] == Some(head) => {
                Some(index + 1).filter(|&next| next < count)
            }
            (Some((index, carets)), false) if carets[focused] == Some(head) => index.checked_sub(1),
            (_, true) => self.diff.next_hunk(side, row),
            (_, false) => self.diff.prev_hunk(side, row),
        };
        let (index, wrapped) = match found {
            Some(index) => (index, None),
            None if forward => (0, Some(Said::WrappedToFirst)),
            None => (count - 1, Some(Said::WrappedToLast)),
        };
        let hunk = self.diff.hunks()[index].clone();
        self.say(wrapped);
        let other = 1 - focused;
        let other_row = hunk.range([Side::Left, Side::Right][other]).start;
        let mut carets = [None; 2];
        // Inline, the left side isn't shown.
        carets[other] = if self.inline {
            None
        } else {
            Some(self.views[other].update(cx, |view, cx| {
                let at = row_offset(view.model().text(), other_row);
                // Out of a visual mode or a pending command, which would put
                // its own selection back.
                view.cancel_vim(cx);
                view.model_mut().set_cursor(at);
                // The focused side scrolls, and this one follows it: across an
                // added or removed hunk the two carets are on different screen
                // rows, so revealing both would pull the sides apart.
                view.autoscroll = false;
                cx.notify();
                view.model().primary_selection().head()
            }))
        };
        carets[focused] = Some(self.views[focused].update(cx, |view, cx| {
            let at = row_offset(view.model().text(), hunk.range(side).start);
            view.select_and_reveal(at..at, cx);
            view.model().primary_selection().head()
        }));
        self.visited = Some((index, carets));
        cx.notify();
    }

    /// What the right header says beside its title: what navigation last
    /// said, or that the texts are too large to compare.
    fn status(&self) -> Option<SharedString> {
        match self.diff.too_large() {
            true => Some(too_large()),
            false => self.message.map(Said::text),
        }
    }

    /// The hunk the focused side's caret is at, as the one last moved to
    /// or else the last starting at or above its row, and the number of
    /// hunks. `None` before the first hunk, or with no hunks.
    fn current_change(&self, window: &Window, cx: &App) -> (Option<usize>, usize) {
        let count = self.diff.hunks().len();
        let focused = match !self.inline && self.views[0].focus_handle(cx).is_focused(window) {
            true => 0,
            false => 1,
        };
        let model = self.views[focused].read(cx).model();
        let head = model.primary_selection().head();
        if let Some((index, carets)) = self.visited
            && carets[focused] == Some(head)
        {
            return (Some(index), count);
        }
        let side = [Side::Left, Side::Right][focused];
        let row = caret_row(model);
        let at = self
            .diff
            .hunks()
            .partition_point(|hunk| hunk.range(side).start <= row);
        (at.checked_sub(1), count)
    }

    /// The previous and next change buttons, and a `Change 2 of 5` count.
    fn change_buttons(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let ui = fe_ui::ui_theme(cx);
        let (current, count) = self.current_change(window, cx);
        let label = (count > 0).then(|| match current {
            Some(index) => tr!("gpui-diff-change-of", index = index + 1, count = count),
            None => tr!("gpui-diff-changes", count = count),
        });
        let button = |id: &'static str, icon, title: SharedString, command: &'static str| {
            IconButton::new(id, icon, title.clone(), &ui)
                .disabled(count == 0)
                .tooltip(move |_, cx| {
                    let shortcut = key_label(command, cx);
                    let tooltip = Tooltip::new(title.clone(), shortcut, &fe_ui::ui_theme(cx));
                    cx.new(|_| tooltip).into()
                })
        };
        let previous = button(
            "previous-change",
            Icon::ArrowLeft,
            tr!("gpui-diff-previous-change"),
            PREVIOUS_CHANGE,
        )
        .on_click(cx.listener(|this, _, window, cx| this.previous_change(window, cx)));
        let next_title = tr!("gpui-diff-next-change");
        let next = button("next-change", Icon::ArrowRight, next_title, NEXT_CHANGE)
            .on_click(cx.listener(|this, _, window, cx| this.next_change(window, cx)));
        div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(2.))
            .px(px(4.))
            .children(label.map(|label| {
                div()
                    .px(px(4.))
                    .debug_selector(|| "change-count".into())
                    .child(label)
            }))
            .child(previous)
            .child(next)
    }

    /// [`NEXT_CHANGE`]: the focused side's caret to the next hunk's start,
    /// wrapping to the first.
    pub fn next_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.go_to_change(true, window, cx);
    }

    /// [`PREVIOUS_CHANGE`]: the focused side's caret to the previous hunk's
    /// start, wrapping to the last.
    pub fn previous_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.go_to_change(false, window, cx);
    }

    /// Runs the diff view's commands; any other goes on to the app.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        match run.0.command.as_str() {
            TOGGLE_INLINE => self.toggle_inline(window, cx),
            TOGGLE_FOLDING => self.toggle_folding(cx),
            NEXT_CHANGE => self.next_change(window, cx),
            PREVIOUS_CHANGE => self.previous_change(window, cx),
            _ => cx.propagate(),
        }
    }
}

/// The single-key shortcut that runs `command` in a diff view, as the
/// keymap has it now.
fn key_label(command: &str, cx: &App) -> Option<String> {
    let command = CommandId::parse(command).ok()?;
    let stack: ContextStack = [KEY_CONTEXT].into_iter().map(ContextFrame::new).collect();
    let keymap = crate::current_keymap(cx);
    fe_keymap::shortcuts(&keymap, &command, &Value::Null, &stack)
        .iter()
        .find(|keys| keys.keystrokes().len() == 1)
        .map(fe_keymap::shortcut_label)
}

/// A diff and the texts it was made from, as diff bases, with their row
/// counts, each side's changed words, and the buffer versions they were
/// read at.
struct Diffed {
    diff: TextDiff,
    texts: [Arc<str>; 2],
    rows: [u32; 2],
    words: [Words; 2],
    versions: [Version; 2],
}

impl Diffed {
    /// Whether either side is over [`WORDS_ASYNC_BYTES`], the size from
    /// which the diff, with its words, is too costly for the main thread.
    fn is_large(left: &Text, right: &Text) -> bool {
        left.len().max(right.len()) > WORDS_ASYNC_BYTES
    }

    fn new(left: &Text, right: &Text, versions: [Version; 2]) -> Self {
        let texts: [Arc<str>; 2] = [left.to_string().into(), right.to_string().into()];
        let diff = TextDiff::new(&texts[0], &texts[1]);
        Self {
            words: Words::of(&diff, &diff.inline(&texts[0], &texts[1])),
            diff,
            rows: texts.each_ref().map(|text| fold::row_count(text)),
            texts,
            versions,
        }
    }
}

/// A side's changed words: its rows to tint whole, and its spans within
/// rows. Rows of the text diffed, which may have moved on.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Words {
    /// The rows with no partner on the other side, sorted: an added or
    /// removed hunk's, and a modified hunk's past the other side's last.
    pub(crate) whole: Vec<Range<u32>>,
    /// The changed bytes within each paired row of a modified hunk, as
    /// `(row, bytes of the row)`, sorted. A pair too long to diff word by
    /// word has none: its rows keep their row tint alone.
    pub(crate) spans: Vec<(u32, Range<usize>)>,
}

impl Words {
    /// Both sides' changed words in `diff`, whose `inline` diff that is.
    fn of(diff: &TextDiff, inline: &InlineDiff) -> [Self; 2] {
        let mut words: [Self; 2] = Default::default();
        for hunk in diff.hunks() {
            let paired = match hunk.kind() {
                HunkKind::Modified => hunk.left.len().min(hunk.right.len()) as u32,
                HunkKind::Added | HunkKind::Removed => 0,
            };
            for (ix, side) in [(0, Side::Left), (1, Side::Right)] {
                let rows = hunk.range(side);
                let pairs_end = rows.start + paired;
                for row in rows.start..pairs_end {
                    let spans = inline.changed_spans(side, row).iter();
                    words[ix]
                        .spans
                        .extend(spans.map(|span| (row, span.clone())));
                }
                if pairs_end < rows.end {
                    words[ix].whole.push(pairs_end..rows.end);
                }
            }
        }
        words
    }

    /// The changed bytes of `row`, in the row: all of its `len` bytes for
    /// a row tinted whole.
    pub(crate) fn of_row(&self, row: u32, len: usize) -> Vec<Range<usize>> {
        let at = self.whole.partition_point(|rows| rows.end <= row);
        if self.whole.get(at).is_some_and(|rows| rows.contains(&row)) {
            return std::iter::once(0..len).collect();
        }
        let from = self.spans.partition_point(|(at, _)| *at < row);
        (self.spans[from..].iter())
            .take_while(|(at, _)| *at == row)
            .map(|(_, span)| span.clone())
            .collect()
    }
}

/// `side`'s rows that differ from the other side's, as its model's row
/// changes: rows only this side has added, rows on both modified, and rows
/// only the other has deleted at this side's start row. None when the
/// texts were too large to compare.
fn row_changes(diff: &TextDiff, side: Side) -> Arc<[RowChange]> {
    let other = match side {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    };
    (diff.hunks().iter())
        .map(|hunk| {
            let (rows, others) = (hunk.range(side).clone(), hunk.range(other));
            let kind = match (rows.is_empty(), others.is_empty()) {
                (false, true) => RowChangeKind::Added,
                (true, _) => RowChangeKind::Deleted,
                (false, false) => RowChangeKind::Modified,
            };
            RowChange { rows, kind }
        })
        .collect()
}

/// The caret's buffer row.
fn caret_row(model: &EditorModel) -> u32 {
    let text = model.text();
    text.offset_to_point(model.primary_selection().head()).row
}

/// The start of `row`, or of the last row past the end.
fn row_offset(text: &Text, row: u32) -> usize {
    text.row_start(row.min(text.row_count().saturating_sub(1)))
}

/// `side`'s rows to tint: removed rows on the left, added rows on the
/// right, modified rows on both.
fn tinted_rows(diff: &TextDiff, side: Side) -> Vec<Range<u32>> {
    (diff.hunks().iter())
        .map(|hunk| hunk.range(side).clone())
        .filter(|rows| !rows.is_empty())
        .collect()
}

/// A side's changed rows as row backgrounds and their changed words as
/// highlights, in the side's tint: [`Tint::DiffRemoved`] on the left,
/// [`Tint::DiffAdded`] on the right, which the theme gives a colour for
/// each.
pub(crate) struct DiffTints {
    tint: Tint,
    /// Rows of the text last diffed, sorted.
    rows: Vec<Range<u32>>,
    words: Arc<Words>,
}

impl DiffTints {
    fn new(tint: Tint) -> Self {
        Self {
            tint,
            rows: Vec::new(),
            words: Default::default(),
        }
    }

    /// The changed words of the rows `range` touches, as highlights: each
    /// cut to its row's text and onto char boundaries, since the text may
    /// have moved on from the one diffed, and none empty.
    fn words(&self, text: &Text, range: &Range<usize>, out: &mut Vec<Decoration>) {
        let count = text.row_count();
        let first = text.offset_to_point(range.start).row;
        let last = text.offset_to_point(range.end).row;
        let mut word = |row: u32, span: Range<usize>| {
            let (start, len) = (text.row_start(row), text.row_len(row) as usize);
            let at = |offset: usize, bias| text.clip_offset(start + offset.min(len), bias);
            let item = at(span.start, Bias::Left)..at(span.end, Bias::Right);
            if !item.is_empty() && overlaps(&item, range) {
                out.push(Decoration::new(item, DecorationKind::Highlight, self.tint));
            }
        };
        let Words { whole, spans } = &*self.words;
        let from = whole.partition_point(|rows| rows.end <= first);
        for rows in whole[from..].iter().take_while(|rows| rows.start <= last) {
            for row in rows.start.max(first)..rows.end.min(last + 1).min(count) {
                word(row, 0..usize::MAX);
            }
        }
        let from = spans.partition_point(|(row, _)| *row < first);
        for (row, span) in spans[from..].iter().take_while(|(row, _)| *row <= last) {
            if *row < count {
                word(*row, span.clone());
            }
        }
    }
}

impl DecorationProvider for DiffTints {
    fn decorations(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        let text = snapshot.text();
        let count = text.row_count();
        // Until a diff of an edit lands, its rows are cut to the text's.
        let start_of = |row: u32| match row < count {
            true => text.row_start(row),
            false => text.len(),
        };
        let tint = &self.tint;
        let first = self
            .rows
            .partition_point(|rows| start_of(rows.end) < range.start);
        for rows in &self.rows[first..] {
            let start = start_of(rows.start);
            if start > range.end {
                break;
            }
            let item = start..start_of(rows.end);
            if overlaps(&item, &range) {
                out.push(Decoration::new(
                    item.clone(),
                    DecorationKind::RowBackground,
                    *tint,
                ));
            }
            // A range ending at the start of the empty last row doesn't
            // cover it: an empty one at the end does.
            let last_empty = text.row_start(count - 1) == text.len();
            let at_end = text.len()..text.len();
            if rows.end >= count && last_empty && !item.is_empty() && overlaps(&at_end, &range) {
                out.push(Decoration::new(
                    at_end,
                    DecorationKind::RowBackground,
                    *tint,
                ));
            }
        }
        self.words(text, &range, out);
    }
}

impl Render for DiffView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // In the locale drawn in.
        self.message_text = self.message.map(Said::text);
        self.start_narrow(window, cx);
        self.keep_aligned(cx);
        self.sync_buffers(cx);
        self.sync_outlines(cx);
        self.show_glyph_column(cx);
        // The sides' theme, as the host last set it.
        let theme = self.views[0].read(cx).theme.clone();
        let mut key_context = KeyContext::default();
        key_context.add(KEY_CONTEXT);
        let header = |title: &SharedString, message: Option<SharedString>| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .gap(px(8.))
                .px(px(8.))
                .py(px(2.))
                .overflow_hidden()
                .whitespace_nowrap()
                .child(div().flex_1().min_w_0().child(title.clone()))
                .children(message.map(|message| div().flex_none().child(message)))
        };
        let buttons = self.change_buttons(window, cx);
        // Inline, the left side isn't shown.
        let labels = [0, 1].map(|ix| (!self.inline || ix == 1).then(|| self.label_layer(ix, cx)));
        let labels = labels.map(Option::flatten);
        let side = |view: &Entity<EditorView>, labels: Option<AnyElement>| {
            (div().relative().flex_1().min_w_0().h_full())
                .child(view.clone())
                .children(labels)
        };
        let [left_labels, right_labels] = labels;
        div()
            .key_context(key_context)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .on_action(cx.listener(Self::on_run_command))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .flex_row()
                    .bg(theme.popover_background)
                    .border_b_1()
                    .border_color(theme.popover_border)
                    .text_color(theme.gutter_active_foreground)
                    .text_size(theme.font_size)
                    .font_family(theme.font_family.clone())
                    .children((!self.inline).then(|| header(&self.titles[0], None)))
                    .child(header(&self.titles[1], self.status()))
                    .child(buttons),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .children((!self.inline).then(|| {
                        side(&self.views[0], left_labels)
                            .border_r_1()
                            .border_color(theme.popover_border)
                    }))
                    .child(side(&self.views[1], right_labels)),
            )
    }
}

impl Focusable for DiffView {
    /// The right side's: usually the one to edit.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.views[1].focus_handle(cx)
    }
}

impl PaneItem for DiffView {
    const KIND: &'static str = "fastedit.diff_view";

    /// `a.rs (Index ↔ Working Tree)`.
    fn tab_title(&self, _: &App) -> SharedString {
        let [left, right] = &self.titles;
        let name = &self.file_name;
        tr!(
            "gpui-diff-tab-title",
            name = name,
            left = left,
            right = right
        )
    }

    fn window_title(&self, cx: &App) -> SharedString {
        let title = self.tab_title(cx);
        tr!(
            "gpui-window-title",
            title = title,
            app = fe_config::APP_NAME
        )
    }

    /// The compared path, else the right side's model's. The view keeps the
    /// default `None` [`PaneItem::path`].
    fn tab_path<'a>(&'a self, cx: &'a App) -> Option<&'a Path> {
        (self.compared_path.as_deref()).or_else(|| self.right_model(cx).path())
    }

    /// Whether the right side's buffer has unsaved changes.
    fn is_dirty(&self, cx: &App) -> bool {
        self.views[1].read(cx).model().is_dirty()
    }

    /// An editor, or another diff view, over the right side's buffer.
    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        let right = self.right_model(cx);
        if let Ok(editor) = other.clone().downcast::<EditorView>() {
            return editor.read(cx).model().shares_buffer_with(right);
        }
        other.clone().downcast::<DiffView>().is_ok_and(|diff| {
            let other = diff.read(cx).right_model(cx);
            other.shares_buffer_with(right)
        })
    }

    /// With the right side's buffer dirty, and no other item showing it,
    /// asks whether to discard its changes, as an editor does.
    fn may_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> CloseAnswer {
        if !self.is_dirty(cx) {
            return CloseAnswer::Close;
        }
        let question = tr!("gpui-close-discard-question", name = &self.file_name);
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some(&tr!("gpui-close-discard-detail")),
            &discard_buttons(),
            cx,
        );
        CloseAnswer::Ask(cx.spawn(async move |_, _| answer.await == Ok(DISCARD)))
    }

    /// Changes with its dirtiness and with every edit to the right side.
    fn close_revision(&self, cx: &App) -> u64 {
        let buffer = self.views[1].read(cx).model().buffer();
        let mut hasher = DefaultHasher::new();
        (buffer.is_dirty(), buffer.id(), buffer.version()).hash(&mut hasher);
        hasher.finish()
    }

    /// Saves the right side, as its editor would.
    fn save(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Task<gpui::Result<()>>> {
        if !self.is_dirty(cx) {
            return None;
        }
        Some(self.views[1].update(cx, |view, cx| EditorView::save(view, window, cx)))
    }

    // Not split or saved in a layout: the host opens it from what it
    // compares.
    fn serialize(&self, _: &App) -> Option<Value> {
        None
    }
}

#[cfg(test)]
#[path = "diff_view_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "diff_view_inline_tests.rs"]
mod diff_view_inline_tests;

#[cfg(test)]
#[path = "diff_view_compare_tests.rs"]
mod diff_view_compare_tests;

#[cfg(test)]
#[path = "diff_view_words_tests.rs"]
mod diff_view_words_tests;

#[cfg(test)]
#[path = "diff_view_change_buttons_tests.rs"]
mod diff_view_change_buttons_tests;

#[cfg(test)]
#[path = "diff_fold_tests.rs"]
mod diff_fold_tests;
