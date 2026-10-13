//! [`DiffView`]: two texts side by side in one kiln-shell pane item, the
//! left (the old text: a file in `HEAD` or the index, say) and the right
//! (the new: usually the file's live buffer). Each side is an
//! [`EditorView`] with a header naming it. Rows line up across the two,
//! the shorter side of each hunk padded with filler blocks
//! ([`TextDiff::fillers`]); removed rows on the left and added rows on the
//! right are tinted, modified rows on both ([`DiffTints`]). Scrolling either
//! side scrolls the other to the same screen row, and `diff.next_change` /
//! `diff.previous_change` move between hunks, wrapping at the ends.
//!
//! The diff is redone when either side's buffer moves on, at most once a
//! frame (on render), off the main thread when a side is over
//! [`ASYNC_BYTES`]; the previous fillers and tints stay until it lands.

use std::{
    cell::RefCell,
    hash::{DefaultHasher, Hash as _, Hasher as _},
    ops::Range,
    rc::Rc,
    sync::Arc,
};

use fe_buffer::{BufferSnapshot, Text, Version};
use fe_command::Value;
use fe_editor::{
    EditorModel,
    decorations::{Decoration, DecorationKind, DecorationProvider, Tint, overlaps},
    text_diff::{HunkKind, Side, TextDiff},
};

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyContext, ParentElement as _, PromptLevel, Render,
    SharedString, Styled as _, Subscription, Task, Window, div, px,
};
use kiln_shell::{CloseAnswer, PaneItem};

use crate::{
    EditorView,
    commands::{EditorViewEvent, HostBlock, HostBlockKind, RunCommand},
    pane_item::{DISCARD_BUTTONS, restore_style},
};

/// Key context active while either side of a diff view is focused.
pub const KEY_CONTEXT: &str = "DiffView";

/// Moves the focused side's caret to the next hunk, wrapping to the first.
pub const NEXT_CHANGE: &str = "diff.next_change";

/// Moves the focused side's caret to the previous hunk, wrapping to the
/// last.
pub const PREVIOUS_CHANGE: &str = "diff.previous_change";

/// A side over this many bytes is diffed off the main thread.
pub const ASYNC_BYTES: usize = 1024 * 1024;

/// What navigation says when it wraps past the last hunk.
pub const WRAPPED_TO_FIRST: &str = "Wrapped to first change";

/// What navigation says when it wraps past the first hunk.
pub const WRAPPED_TO_LAST: &str = "Wrapped to last change";

/// What navigation says when the texts are the same.
pub const NO_CHANGES: &str = "No changes";

/// What the header and navigation say when a side is over
/// [`text_diff::MAX_DIFF_BYTES`](fe_editor::text_diff::MAX_DIFF_BYTES), so
/// nothing was compared.
pub const TOO_LARGE: &str = "Too large to compare";

/// How often the view looks for edits made through a model it doesn't
/// show, such as the file's editor in another window, which redraw
/// neither it nor its sides.
const POLL: std::time::Duration = std::time::Duration::from_millis(250);

/// The answer to [`DISCARD_BUTTONS`] that closes.
const DISCARD: usize = 1;

/// One side of a [`DiffView`]: its header's title (`HEAD`, `Index`,
/// `Working Tree`, a path) and the model it shows.
pub struct DiffSide {
    pub title: SharedString,
    pub model: EditorModel,
    /// Whether the side refuses edits. The left side always does.
    pub read_only: bool,
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
    titles: [SharedString; 2],
    views: [Entity<EditorView>; 2],
    /// Each side's tints, registered on its model.
    tints: [Rc<RefCell<DiffTints>>; 2],
    /// Whether each side takes the other's text as its diff base: a
    /// read-only side's model is the host's own, over a fixed text.
    sets_base: [bool; 2],
    /// The diff the fillers and tints show.
    diff: TextDiff,
    /// The buffer versions last diffed, or being diffed.
    diffed: Option<[Version; 2]>,
    /// A diff running off the main thread.
    diffing: Option<Task<()>>,
    /// What navigation last said about the diff shown, in the header.
    message: Option<SharedString>,
    /// Whether the right side's buffer was dirty when last looked at: a
    /// save elsewhere changes it without changing the text.
    dirty: bool,
    /// The hunk navigation last went to, and where it put each side's
    /// caret: while the focused side's caret is still there, the next move
    /// counts from that hunk, so one at the end of a side, whose start is
    /// past the last row, isn't taken for the row the caret's on.
    visited: Option<(usize, [usize; 2])>,
    _subscriptions: Vec<Subscription>,
    /// Looks for edits from elsewhere every [`POLL`].
    _poll: Task<()>,
    /// How many times each side's scroll top was set from the other's.
    #[cfg(test)]
    pub(crate) scroll_sets: [usize; 2],
}

impl DiffView {
    /// Compares `left` with `right`, in the theme and settings of
    /// [`crate::set_restore_style`]. The left side is made read-only.
    pub fn new(
        file_name: impl Into<SharedString>,
        left: DiffSide,
        right: DiffSide,
        cx: &mut Context<Self>,
    ) -> Self {
        let (theme, settings) = restore_style(cx);
        let tints = [
            Rc::new(RefCell::new(DiffTints::default())),
            Rc::new(RefCell::new(DiffTints::default())),
        ];
        let sets_base = [true, right.read_only];
        let titles = [left.title, right.title];
        let mut view = |mut model: EditorModel, read_only: bool, tints: &Rc<RefCell<DiffTints>>| {
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
        let mut subscriptions = Vec::new();
        for (from, to) in [(0, 1), (1, 0)] {
            // A side's edits, carets and scrolls may change the diff.
            subscriptions.push(cx.observe(&views[from], |_, _, cx| cx.notify()));
            subscriptions.push(cx.subscribe(
                &views[from],
                move |this: &mut Self, _, event: &EditorViewEvent, cx| match *event {
                    EditorViewEvent::Scrolled { top } => this.follow(to, top, cx),
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
            file_name: file_name.into(),
            titles,
            views,
            tints,
            sets_base,
            diff: TextDiff::default(),
            diffed: None,
            diffing: None,
            message: None,
            dirty: false,
            visited: None,
            _subscriptions: subscriptions,
            _poll: poll,
            #[cfg(test)]
            scroll_sets: [0; 2],
        };
        this.sync_buffers(cx);
        this
    }

    pub fn left(&self) -> &Entity<EditorView> {
        &self.views[0]
    }

    pub fn right(&self) -> &Entity<EditorView> {
        &self.views[1]
    }

    /// The diff the fillers and tints show now.
    pub fn diff(&self) -> &TextDiff {
        &self.diff
    }

    /// What navigation last said: that it wrapped, found no changes, or
    /// that the texts are too large to compare.
    pub fn message(&self) -> Option<&SharedString> {
        self.message.as_ref()
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
            let diffed = Diffed::new(&left, &right);
            self.land(diffed, cx);
            return;
        }
        let diffing = cx.background_spawn(async move { Diffed::new(&left, &right) });
        self.diffing = Some(cx.spawn(async move |this, cx| {
            let diffed = diffing.await;
            this.update(cx, |this, cx| {
                this.diffing = None;
                this.land(diffed, cx);
            })
            .ok();
        }));
    }

    /// Shows `diffed`: each side's fillers and tints, and the other side's
    /// text as the diff base of a side that takes it. Not over
    /// [`ASYNC_BYTES`]: the model would diff against it on the main thread,
    /// as a frame asks for its change bars.
    fn land(&mut self, diffed: Diffed, cx: &mut Context<Self>) {
        let Diffed { diff, texts } = diffed;
        let large = texts.iter().any(|text| text.len() > ASYNC_BYTES);
        for (ix, side) in [(0, Side::Left), (1, Side::Right)] {
            self.tints[ix].borrow_mut().rows = tinted_rows(&diff, side);
            let blocks = (diff.fillers(side).into_iter().enumerate())
                .map(|(id, (above, rows))| HostBlock {
                    above,
                    rows,
                    kind: HostBlockKind::Filler,
                    id: id as u64,
                })
                .collect();
            let base = (!large).then(|| texts[1 - ix].clone());
            let sets_base = self.sets_base[ix];
            self.views[ix].update(cx, |view, cx| {
                if sets_base {
                    view.model_mut().set_diff_base(base);
                }
                view.set_blocks(blocks, cx);
                // The tints changed.
                cx.notify();
            });
        }
        self.diff = diff;
        // What navigation said was about the last diff.
        self.message = None;
        self.visited = None;
        cx.notify();
    }

    /// Keeps both sides unwrapped and unfolded: fillers line up buffer
    /// rows, so a row wrapped or folded on one side alone would put the
    /// rows below it out of line.
    fn keep_aligned(&mut self, cx: &mut Context<Self>) {
        for view in &self.views {
            let model = view.read(cx).model();
            if model.soft_wrap() || !model.display().folds().is_empty() {
                view.update(cx, |view, cx| {
                    let model = view.model_mut();
                    model.set_soft_wrap(false);
                    model.unfold_all();
                    cx.notify();
                });
            }
        }
    }

    /// Moves the focused side's caret to the start of the next hunk
    /// (`forward`) or the previous one, wrapping at the ends; the other
    /// side's caret goes to the hunk's start there, and its scroll follows.
    fn go_to_change(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let focused = match self.views[0].focus_handle(cx).is_focused(window) {
            true => 0,
            false => 1,
        };
        let side = [Side::Left, Side::Right][focused];
        let model = self.views[focused].read(cx).model();
        let (row, head) = (caret_row(model), model.primary_selection().head());
        let count = self.diff.hunks().len();
        if count == 0 {
            let message = match self.diff.too_large() {
                true => TOO_LARGE,
                false => NO_CHANGES,
            };
            self.message = Some(message.into());
            cx.notify();
            return;
        }
        let found = match (self.visited, forward) {
            (Some((index, carets)), true) if carets[focused] == head => {
                Some(index + 1).filter(|&next| next < count)
            }
            (Some((index, carets)), false) if carets[focused] == head => index.checked_sub(1),
            (_, true) => self.diff.next_hunk(side, row),
            (_, false) => self.diff.prev_hunk(side, row),
        };
        let (index, wrapped) = match found {
            Some(index) => (index, None),
            None if forward => (0, Some(WRAPPED_TO_FIRST)),
            None => (count - 1, Some(WRAPPED_TO_LAST)),
        };
        let hunk = self.diff.hunks()[index].clone();
        self.message = wrapped.map(SharedString::from);
        let other = 1 - focused;
        let other_row = hunk.range([Side::Left, Side::Right][other]).start;
        let mut carets = [0; 2];
        carets[other] = self.views[other].update(cx, |view, cx| {
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
        });
        carets[focused] = self.views[focused].update(cx, |view, cx| {
            let at = row_offset(view.model().text(), hunk.range(side).start);
            view.select_and_reveal(at..at, cx);
            view.model().primary_selection().head()
        });
        self.visited = Some((index, carets));
        cx.notify();
    }

    /// What the right header says beside its title: what navigation last
    /// said, or that the texts are too large to compare.
    fn status(&self) -> Option<SharedString> {
        match self.diff.too_large() {
            true => Some(TOO_LARGE.into()),
            false => self.message.clone(),
        }
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
            NEXT_CHANGE => self.next_change(window, cx),
            PREVIOUS_CHANGE => self.previous_change(window, cx),
            _ => cx.propagate(),
        }
    }
}

/// A diff and the texts it was made from, as diff bases.
struct Diffed {
    diff: TextDiff,
    texts: [Arc<str>; 2],
}

impl Diffed {
    /// Whether either side is over [`ASYNC_BYTES`].
    fn is_large(left: &Text, right: &Text) -> bool {
        left.len().max(right.len()) > ASYNC_BYTES
    }

    fn new(left: &Text, right: &Text) -> Self {
        let texts: [Arc<str>; 2] = [left.to_string().into(), right.to_string().into()];
        Self {
            diff: TextDiff::new(&texts[0], &texts[1]),
            texts,
        }
    }
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
fn tinted_rows(diff: &TextDiff, side: Side) -> Vec<(Range<u32>, Tint)> {
    (diff.hunks().iter())
        .filter(|hunk| !hunk.range(side).is_empty())
        .map(|hunk| {
            let tint = match hunk.kind() {
                HunkKind::Added => Tint::Added,
                HunkKind::Removed => Tint::Removed,
                HunkKind::Modified => Tint::Modified,
            };
            (hunk.range(side).clone(), tint)
        })
        .collect()
}

/// A side's changed rows as row backgrounds, in the tint of their change.
#[derive(Default)]
pub(crate) struct DiffTints {
    /// Rows of the text last diffed, sorted.
    rows: Vec<(Range<u32>, Tint)>,
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
        let first = self
            .rows
            .partition_point(|(rows, _)| start_of(rows.end) < range.start);
        for (rows, tint) in &self.rows[first..] {
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
    }
}

impl Render for DiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.keep_aligned(cx);
        self.sync_buffers(cx);
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
        let side =
            |view: &Entity<EditorView>| div().flex_1().min_w_0().h_full().child(view.clone());
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
                    .child(header(&self.titles[0], None))
                    .child(header(&self.titles[1], self.status())),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(
                        side(&self.views[0])
                            .border_r_1()
                            .border_color(theme.popover_border),
                    )
                    .child(side(&self.views[1])),
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
        format!("{} ({left} ↔ {right})", self.file_name).into()
    }

    fn window_title(&self, cx: &App) -> SharedString {
        format!("{} — fastedit", self.tab_title(cx)).into()
    }

    /// Whether the right side's buffer has unsaved changes.
    fn is_dirty(&self, cx: &App) -> bool {
        self.views[1].read(cx).model().is_dirty()
    }

    /// An editor, or another diff view, over the right side's buffer.
    fn shares_document(&self, other: &AnyView, cx: &App) -> bool {
        let right = self.views[1].read(cx).model();
        if let Ok(editor) = other.clone().downcast::<EditorView>() {
            return editor.read(cx).model().shares_buffer_with(right);
        }
        other.clone().downcast::<DiffView>().is_ok_and(|diff| {
            let other = diff.read(cx).views[1].read(cx).model();
            other.shares_buffer_with(right)
        })
    }

    /// With the right side's buffer dirty, and no other item showing it,
    /// asks whether to discard its changes, as an editor does.
    fn may_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> CloseAnswer {
        if !self.is_dirty(cx) {
            return CloseAnswer::Close;
        }
        let question = format!("Discard unsaved changes to {}?", self.file_name);
        let answer = window.prompt(
            PromptLevel::Warning,
            &question,
            Some("Your changes will be lost if you close it without saving."),
            &DISCARD_BUTTONS,
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
