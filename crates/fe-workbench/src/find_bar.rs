//! A pane's find bar: a row under the pane's toolbar that finds in the
//! pane's active editor, with a query, three options and an `n of m` count.
//!
//! Each change to the query or an option builds a [`Pattern`] from the
//! [`FindQuery`] and sets it on the editor ([`EditorView::set_find`]), which
//! highlights and counts its matches, then selects the first match at or
//! after the anchor: the selection's start when the bar was shown, moved to
//! each match `find.next` and `find.previous` step to, and to the
//! selection's start whenever the selection has moved since the bar last
//! left it, by a click, an edit or a reveal. While the bar is open
//! the pattern follows the pane's active tab; closed, it's on no editor, and
//! `find.next` and `find.previous` still step with the last query.
//!
//! A second row under the query, hidden until `find.show_replace` or the
//! chevron at the bar's left shows it, replaces with the same query and
//! options: `find.replace_next` replaces the match the primary selection is
//! (with vim on, the one at the caret) and selects the next, and
//! `find.replace_all` replaces every match in one edit, so one undo takes
//! it all back. The replacement is literal unless the regex option is on,
//! when `$1` and the like expand ([`Replacement::parse`]). With Preserve
//! Case on (`find.toggle_preserve_case`, the `AB` toggle in the replace
//! row), each match's replacement, expanded, then takes the match's case
//! ([`preserve_case::apply`]).
//!
//! Find in Selection (`find.toggle_in_selection`) limits all of that to the
//! editor's non-empty selections when it's turned on: only matches wholly
//! inside them are found, counted and replaced, and they're drawn as a band
//! ([`EditorView::set_find_scoped`]). If the bar's own finding has moved the
//! selection off the ones the user made, as a remembered query's does when the
//! bar opens, it's those. With no such selection it stays off. Turning it off,
//! closing the bar or the bar's following another editor drops them; an edit
//! inside moves their ends.
//!
//! The bar draws as the design's find row (`docs/design/design-system.md`,
//! Find and replace): the query in a bordered field with the accent focus
//! ring, Match Case, Whole Word and Regex as icon toggles inside it, then the
//! count, Find Previous, Find Next and Find in Selection; the replace row's
//! field holds Preserve Case, then Replace and Replace All. With vim on, the
//! equivalent Vim command shows faintly at the row's right end
//! ([`FindBar::vim_command`]), and a click copies it, as Enter or Space
//! does once the keyboard has moved focus to it: it's in the tab order.
//!
//! To a screen reader the bar is a `Search` landmark named "Find", its
//! query a `SearchInput` and its replacement a `TextInput`, each named by
//! its placeholder; its toggles and buttons are named by their tooltips,
//! and the Vim command is a button. The count is announced, politely:
//! [`ANNOUNCE_DELAY`] after the last change to the query, and at once
//! after Find Next, Find Previous or an option's toggle, as soon as it's
//! counted ([`FindBar::announce_label`]). With the bar closed, Find Next
//! and Previous still step with the last query, and the count of the match
//! they reach is counted for the announcement alone.
//!
//! The commands are the pane's typed commands ([`crate::find_commands`]),
//! which its root runs ([`crate::Pane::run_find`]). The bar's element sets
//! [`KEY_CONTEXT`] and its inputs `text_input`, the replace input
//! [`REPLACE_KEY_CONTEXT`] too, and none carries `vim.mode`, so with vim on keys typed there only type.

use std::{cell::Cell, ops::Range, time::Duration};

use fe_buffer::{Anchor, BufferId, Origin, Text};
use fe_editor::Movement;
use fe_gpui::{EditorView, commands::RunCommand};
use fe_ui::{
    ActiveTheme as _, Button, FamilySlot, FontFamilies, Icon, IconButton, TextInput,
    TextInputEvent, ToggleButton, Tooltip, UiTheme, focus_ring, node, radius,
};
use fe_vim::search::{
    Deadline, FindQuery, Interrupted, Pattern, PatternError, matches_reaching_until,
    replace::{Replacement, replace_all, replace_where},
};
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext as _, ClipboardItem, Context, Div, Entity,
    FocusHandle, Focusable as _, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};
use kiln_l10n::{text, tr};

use crate::{
    Pane,
    picker::{self, PaletteOp},
    preserve_case,
};

/// The key context the find bar's element sets.
pub(crate) const KEY_CONTEXT: &str = "find_bar";

/// The key context the replace input's element adds, inside the bar's.
pub(crate) const REPLACE_KEY_CONTEXT: &str = "replace_input";

/// The width of the column the chevron that shows the replace row is in.
const CHEVRON_WIDTH: f32 = 28.;

/// The find and replace fields' width, which they shrink from, to
/// [`FIELD_MIN_WIDTH`], only when the bar is too narrow for it.
const FIELD_WIDTH: f32 = 360.;
const FIELD_MIN_WIDTH: f32 = 160.;

/// A range of the text and what replaces it.
type Edit = (Range<usize>, String);

/// What [`FindBar::replace_next`] does: select a match, or none; or make
/// a replacement and select the match after it, in the text as that leaves
/// it.
enum Step {
    Select(Option<Range<usize>>),
    Replace(Edit, Option<Range<usize>>),
}

/// What a find command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FindOp {
    Show,
    Next,
    Previous,
    Close,
    ToggleCase,
    ToggleWholeWord,
    ToggleRegex,
    ToggleInSelection,
    ToggleReplace,
    ShowReplace,
    ReplaceNext,
    ReplaceAll,
}

/// How long after the last change to the query the count is announced:
/// a screen reader isn't told a count for every key.
pub(crate) const ANNOUNCE_DELAY: Duration = Duration::from_millis(300);

/// A pane's find bar: see the module docs.
pub(crate) struct FindBar {
    open: bool,
    input: Entity<TextInput>,
    /// Whether the replace row shows.
    replacing: bool,
    replace_input: Entity<TextInput>,
    /// What the label says in place of the count after a replace all, and
    /// whether it's an error, until the next find command or query change:
    /// resolved as it's drawn, so in the locale active then.
    status: Option<(kiln_l10n::Text, bool)>,
    match_case: bool,
    whole_word: bool,
    regex: bool,
    /// Whether a replacement takes the case of the match it replaces. It
    /// changes no match, so toggling it doesn't search again.
    preserve_case: bool,
    /// The selections Find in Selection captured, in the buffer of
    /// [`FindBar::editor`]; `None` while it's off.
    scope: Option<Vec<Range<Anchor>>>,
    /// The non-empty selections the user last made, which the bar's finding
    /// may have moved off, for Find in Selection: see
    /// [`FindBar::note_selections`].
    selected: Option<Vec<Range<Anchor>>>,
    /// What the query makes; `None` while it's empty.
    pattern: Option<Result<Pattern, PatternError>>,
    /// Where a change to the query or an option finds from, in the buffer
    /// of the editor last found in, so edits move it: see [`FindBar::from`].
    anchor: Option<(BufferId, Anchor)>,
    /// The primary selection as the bar last left it, in that buffer: one
    /// that has moved since is the user's, and its start the anchor.
    left: Option<(BufferId, Range<Anchor>)>,
    /// All the selections as the bar last left them, in that buffer.
    left_all: Option<(BufferId, Vec<Range<Anchor>>)>,
    /// The editor the bar set its pattern on, while it's open.
    editor: Option<WeakEntity<EditorView>>,
    /// The Vim command's, kept across frames so the keyboard can move
    /// focus to it.
    vim_command_focus: FocusHandle,
    /// The label is to be announced once it shows a count or an error.
    announce: Cell<bool>,
    /// Asks for the announcement [`ANNOUNCE_DELAY`] after the query's last
    /// change: a newer change replaces it.
    announce_after: Option<Task<()>>,
    /// The window the bar was shown in, its pane's: its count is said only
    /// while that's the active window.
    window: Option<AnyWindowHandle>,
    _input: Subscription,
}

impl FindBar {
    pub(crate) fn new(ui: &UiTheme, cx: &mut Context<Pane>) -> Self {
        // A placeholder is resolved once, as a picker's is.
        let input = cx.new(|cx| TextInput::new(tr!("workbench-find-placeholder"), ui, cx).search());
        let replace_input =
            cx.new(|cx| TextInput::new(tr!("workbench-find-replace-placeholder"), ui, cx));
        let changed = cx.subscribe(&input, |pane, _, event, cx| {
            if *event == TextInputEvent::Changed {
                pane.update_find(|bar, editor, cx| bar.search(editor, cx), cx);
                let after = cx.spawn(async |pane, cx| {
                    cx.background_executor().timer(ANNOUNCE_DELAY).await;
                    (pane.update(cx, |pane, cx| {
                        pane.update_find(|bar, _, _| bar.announce.set(true), cx);
                        cx.notify();
                    }))
                    .ok();
                });
                // A count owed for the query before waits for this one's.
                pane.update_find(
                    |bar, _, _| {
                        bar.announce.set(false);
                        bar.announce_after = Some(after);
                    },
                    cx,
                );
                cx.notify();
            }
        });
        Self {
            open: false,
            input,
            replacing: false,
            replace_input,
            status: None,
            match_case: false,
            whole_word: false,
            regex: false,
            preserve_case: false,
            scope: None,
            selected: None,
            pattern: None,
            anchor: None,
            left: None,
            left_all: None,
            editor: None,
            vim_command_focus: cx.focus_handle().tab_stop(true),
            announce: Cell::new(false),
            announce_after: None,
            window: None,
            _input: changed,
        }
    }

    #[cfg(test)]
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    #[cfg(test)]
    pub(crate) fn input(&self) -> &Entity<TextInput> {
        &self.input
    }

    #[cfg(test)]
    pub(crate) fn is_replacing(&self) -> bool {
        self.replacing
    }

    #[cfg(test)]
    pub(crate) fn replace_input(&self) -> &Entity<TextInput> {
        &self.replace_input
    }

    #[cfg(test)]
    pub(crate) fn preserves_case(&self) -> bool {
        self.preserve_case
    }

    #[cfg(test)]
    pub(crate) fn in_selection(&self) -> bool {
        self.scope.is_some()
    }

    /// Flips Preserve Case, which the next replace follows.
    pub(crate) fn toggle_preserve_case(&mut self) {
        self.status = None;
        self.preserve_case = !self.preserve_case;
    }

    /// The query the input and options make.
    pub(crate) fn query(&self, cx: &App) -> FindQuery {
        FindQuery {
            text: self.input.read(cx).text().to_owned(),
            regex: self.regex,
            match_case: self.match_case,
            whole_word: self.whole_word,
        }
    }

    /// Does `op` in `editor`, the pane's active one, if any.
    pub(crate) fn run(
        &mut self,
        op: FindOp,
        editor: Option<&Entity<EditorView>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if op != FindOp::ToggleReplace {
            self.status = None;
        }
        match op {
            FindOp::Show => self.show(false, editor, window, cx),
            FindOp::ShowReplace => self.show(true, editor, window, cx),
            FindOp::Next => self.step(true, editor, cx),
            FindOp::Previous => self.step(false, editor, cx),
            FindOp::Close => {
                self.open = false;
                self.announce.set(false);
                self.announce_after = None;
                self.scope = None;
                self.selected = None;
                self.set_editor(None, cx);
            }
            FindOp::ToggleCase => self.toggle(|bar| &mut bar.match_case, editor, cx),
            FindOp::ToggleWholeWord => self.toggle(|bar| &mut bar.whole_word, editor, cx),
            FindOp::ToggleRegex => self.toggle(|bar| &mut bar.regex, editor, cx),
            FindOp::ToggleInSelection => self.toggle_in_selection(editor, cx),
            FindOp::ToggleReplace => self.toggle_replace(window, cx),
            FindOp::ReplaceNext => self.replace_next(editor, cx),
            FindOp::ReplaceAll => self.replace_all(editor, cx),
        }
    }

    /// Opens the bar, or keeps it open, with focus in its input, or with
    /// `replace` in the replace input, the replace row shown. A non-empty
    /// selection within one row replaces the query, and the selection's
    /// start is the anchor.
    fn show(
        &mut self,
        replace: bool,
        editor: Option<&Entity<EditorView>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.open = true;
        self.window = Some(window.window_handle());
        self.replacing |= replace;
        self.set_editor(editor, cx);
        if let Some(editor) = editor {
            editor.update(cx, |editor, _| self.note_selections(editor));
            let view = editor.read(cx);
            let selection = view.model().primary_selection().range;
            let text = view.model().text();
            let one_row = text.offset_to_point(selection.start).row
                == text.offset_to_point(selection.end).row;
            let selected =
                (!selection.is_empty() && one_row).then(|| text.slice_to_string(selection.clone()));
            self.leave(view, true);
            if let Some(selected) = selected {
                self.input
                    .update(cx, |input, cx| input.set_text(selected, cx));
            }
        }
        self.search(editor, cx);
        let input = if replace {
            &self.replace_input
        } else {
            &self.input
        };
        window.focus(&input.focus_handle(cx), cx);
    }

    /// Turns Find in Selection on or off, and finds again from the anchor.
    /// On, it captures the non-empty selections the user last made
    /// ([`FindBar::selected`]), or else `editor`'s, which are the bar's own
    /// finding's. With neither it stays off and nothing else changes.
    fn toggle_in_selection(&mut self, editor: Option<&Entity<EditorView>>, cx: &mut App) {
        if !self.open {
            return;
        }
        if self.scope.take().is_none() {
            let Some(editor) = editor else {
                return;
            };
            self.scope = editor.update(cx, |editor, _| {
                self.note_selections(editor);
                self.selected.clone().or_else(|| editor.selection_scope())
            });
            if self.scope.is_none() {
                return;
            }
        }
        self.search(editor, cx);
        self.announce_now();
    }

    /// Before the bar moves the selection: if any selection has changed since
    /// the bar left them, they're the user's, to remember for Find in
    /// Selection. With the bar closed there's nothing to remember them for.
    fn note_selections(&mut self, editor: &mut EditorView) {
        if !self.open {
            return;
        }
        let left = self.left_all.as_ref().and_then(|(buffer, ranges)| {
            (ranges.iter())
                .map(|range| {
                    Some(
                        resolve(*buffer, &range.start, editor)?
                            ..resolve(*buffer, &range.end, editor)?,
                    )
                })
                .collect::<Option<Vec<_>>>()
        });
        let now = (editor.model().selections().into_iter())
            .map(|selection| selection.range)
            .collect();
        if left != Some(now) {
            self.selected = editor.selection_scope();
        }
    }

    /// The scope in `editor`'s text as it is, with it on: empty rather than
    /// `None` if `editor` hasn't got it, so a replace never goes outside it.
    fn scope_in(&self, editor: &EditorView) -> Option<Vec<Range<usize>>> {
        (self.scope.as_ref()).map(|_| editor.find_scope().unwrap_or_default())
    }

    /// Shows or hides the replace row. Hiding it with focus in the replace
    /// input moves focus to the query.
    fn toggle_replace(&mut self, window: &mut Window, cx: &mut App) {
        self.replacing = !self.replacing;
        if !self.replacing && self.replace_input.focus_handle(cx).is_focused(window) {
            window.focus(&self.input.focus_handle(cx), cx);
        }
    }

    /// The replace input's text as a replacement for the query's matches.
    fn replacement(&self, cx: &App) -> Replacement {
        Replacement::parse(self.replace_input.read(cx).text(), self.regex)
    }

    /// Replaces the match the primary selection is, with vim on the one at
    /// the caret, and selects the next match after it; if it's no match,
    /// only selects the next match from the anchor. The matches are those
    /// [`replace_all`] replaces ([`next_match`]), so each one selected can
    /// be replaced, and an empty one replaced is stepped past. Both are
    /// found before anything changes, the next in the text as the replace
    /// will leave it: if that runs out of time, the label says so and the
    /// text, selection and vim's mode stay as they are. An open vim insert or
    /// replace session stays open ([`apply`]). With Find in Selection on, a
    /// selected match outside the scope is no match, and the next is in it.
    fn replace_next(&mut self, editor: Option<&Entity<EditorView>>, cx: &mut App) {
        let (Some(editor), Some(pattern)) = (editor, self.valid_pattern()) else {
            return;
        };
        if !self.open {
            return;
        }
        let with = self.replacement(cx);
        editor.update(cx, |editor, cx| {
            // Catching up with edits through another view of the buffer
            // first, so the offsets read are the text's as it is.
            editor.model_mut().sync_display();
            self.note_selections(editor);
            let step = match self.plan_next(editor, &pattern, &with, self.preserve_case) {
                Ok(step) => step,
                Err(Interrupted) => {
                    self.status = Some((text!("workbench-find-too-long"), true));
                    return;
                }
            };
            let (edit, next) = match step {
                Step::Select(next) => {
                    match next {
                        Some(next) => select_match(editor, next, cx),
                        None => editor.leave_vim_visual_mode(cx),
                    }
                    self.leave(editor, true);
                    return;
                }
                Step::Replace(edit, next) => (edit, next),
            };
            // The replace makes the text the next match was found in.
            apply(editor, vec![edit], cx);
            if let Some(next) = next {
                select_match(editor, next, cx);
            }
            self.leave(editor, true);
        });
    }

    /// What [`FindBar::replace_next`] does in `editor`'s text as it is: the
    /// match to replace, if the selection is one, with its replacement, and
    /// the match to select next, after it in the text as replacing it
    /// leaves it; or, if there's none to replace, the next match from the
    /// anchor. Each is searched for, and the replacement made, under one
    /// deadline.
    fn plan_next(
        &mut self,
        editor: &EditorView,
        pattern: &Pattern,
        with: &Replacement,
        preserve_case: bool,
    ) -> Result<Step, Interrupted> {
        let mut deadline = Deadline::new();
        let scope = self.scope_in(editor);
        let current = match editor.is_read_only() {
            true => None,
            false => current_match(
                editor,
                pattern,
                with,
                preserve_case,
                scope.as_deref(),
                &mut deadline,
            )?,
        };
        let text = editor.model().text();
        let Some((range, new)) = current else {
            let from = self.from(editor);
            let scope = scope.as_deref();
            let next = next_match(
                editor.model().text(),
                pattern,
                from,
                true,
                scope,
                &mut deadline,
            )?;
            return Ok(Step::Select(next));
        };
        let mut replaced = text.clone();
        replaced.replace(range.clone(), &new);
        let end = range.start + new.len();
        let scope = scope.map(|scope| scope_after(scope, &range, new.len()));
        let empty_at_end = !range.is_empty();
        let next = next_match(
            &replaced,
            pattern,
            end,
            empty_at_end,
            scope.as_deref(),
            &mut deadline,
        )?;
        Ok(Step::Replace((range, new), next))
    }

    /// Replaces every match, with Find in Selection on every one inside the
    /// scope, in one edit, the caret after the last, and says how many in the
    /// label, or that the search took too long, replacing none and changing
    /// nothing. An open vim insert or replace session stays open ([`apply`]).
    fn replace_all(&mut self, editor: Option<&Entity<EditorView>>, cx: &mut App) {
        let (Some(editor), Some(pattern)) = (editor, self.valid_pattern()) else {
            return;
        };
        if !self.open || editor.read(cx).is_read_only() {
            return;
        }
        let with = self.replacement(cx);
        editor.update(cx, |editor, cx| {
            editor.model_mut().sync_display();
            self.note_selections(editor);
            let scope = self.scope_in(editor);
            let text = editor.model().text();
            let found = match &scope {
                Some(scope) => replace_where(
                    text,
                    EditorView::find_scope_rows(text, scope),
                    &pattern,
                    &with,
                    |found| EditorView::in_find_scope(text, scope, found),
                ),
                None => replace_all(text, &pattern, &with),
            };
            let edits = match found {
                Ok(edits) if self.preserve_case => (edits.into_iter())
                    .map(|(range, new)| {
                        let new = preserve_case::apply(&text.slice_to_string(range.clone()), &new);
                        (range, new)
                    })
                    .collect(),
                Ok(edits) => edits,
                // Nothing changes, vim's mode and selection neither.
                Err(Interrupted) => {
                    self.status = Some((text!("workbench-find-too-long"), true));
                    return;
                }
            };
            let count = edits.len();
            if count == 0 {
                editor.leave_vim_visual_mode(cx);
            } else {
                apply(editor, edits, cx);
                self.leave(editor, true);
            }
            self.status = Some((text!("workbench-find-replaced", count = count), false));
        });
    }

    /// The input that has focus: the replace input, or else the query.
    fn focused_input(&self, window: &Window, cx: &App) -> &Entity<TextInput> {
        if self.replace_input.focus_handle(cx).is_focused(window) {
            &self.replace_input
        } else {
            &self.input
        }
    }

    /// Builds the pattern, sets it on `editor` and selects the first match
    /// from the anchor. An empty or invalid query clears the editor's find.
    fn search(&mut self, editor: Option<&Entity<EditorView>>, cx: &mut App) {
        self.status = None;
        let query = self.query(cx);
        self.pattern = (!query.text.is_empty()).then(|| Pattern::from_query(&query));
        if !self.open {
            return;
        }
        let Some(editor) = editor else {
            return;
        };
        let pattern = self.valid_pattern();
        editor.update(cx, |editor, cx| {
            self.note_selections(editor);
            let found = pattern.is_some();
            editor.set_find_scoped(pattern, self.scope.clone(), cx);
            if found {
                let from = self.from(editor);
                editor.find_from(from, cx);
            }
            self.leave(editor, false);
        });
    }

    fn valid_pattern(&self) -> Option<Pattern> {
        self.pattern.as_ref()?.as_ref().ok().cloned()
    }

    /// The next match, or the previous one, wrapping, which becomes the
    /// anchor. With the bar closed it's found with the last query, and
    /// nothing stays highlighted.
    fn step(&mut self, forward: bool, editor: Option<&Entity<EditorView>>, cx: &mut App) {
        let Some(editor) = editor else {
            return;
        };
        let pattern = match self.open {
            true => None,
            false => match self.valid_pattern() {
                Some(pattern) => Some(pattern),
                None => return,
            },
        };
        let closed = editor.update(cx, |editor, cx| {
            self.note_selections(editor);
            let transient = pattern.is_some();
            if transient {
                editor.set_find(pattern.clone(), cx);
            }
            if forward {
                editor.find_next(cx);
            } else {
                editor.find_previous(cx);
            }
            if transient {
                editor.set_find(None, cx);
            }
            self.leave(editor, true);
            let model = editor.model();
            let selection = model.primary_selection();
            // With vim on, the match is the one at the caret.
            let at = match editor.vim_mode() {
                Some(_) => selection.head()..selection.head(),
                None => selection.range,
            };
            pattern.map(|pattern| (pattern, model.text().clone(), at))
        });
        match closed {
            None => self.announce_now(),
            Some((pattern, text, at)) => {
                let source = Source {
                    editor: editor.downgrade(),
                    window: cx.active_window(),
                    version: editor.read(cx).model().snapshot().version(),
                    at,
                };
                self.announce_closed_count(pattern, text, source, cx)
            }
        }
    }

    /// With the bar closed, the count isn't drawn: it's counted here for the
    /// match Find Next or Previous left at `source`'s `at` (or, collapsed,
    /// starting there), and announced once it's known, if it's still the
    /// count of what the user is on ([`Source::stands`]). The next step
    /// replaces it.
    fn announce_closed_count(
        &mut self,
        pattern: Pattern,
        text: Text,
        source: Source,
        cx: &mut App,
    ) {
        self.announce.set(false);
        let at = source.at.clone();
        let counting = cx.background_spawn(async move {
            let (ranges, capped) = count_matches(&text, &pattern);
            let current = ranges
                .binary_search_by_key(&at.start, |range| range.start)
                .ok()
                .filter(|&ix| at.is_empty() || ranges[ix] == at);
            (ranges.len(), current.map(|ix| ix + 1), capped)
        });
        self.announce_after = Some(cx.spawn(async move |cx| {
            let (total, current, capped) = counting.await;
            cx.update(|cx| {
                if source.stands(cx) {
                    let said = count_text(total, current, capped);
                    fe_ui::announce(cx, said, fe_ui::Politeness::Polite);
                }
            });
        }));
    }

    /// Has the label announced as soon as it shows a count, not after the
    /// query's delay.
    fn announce_now(&mut self) {
        self.announce.set(true);
        self.announce_after = None;
    }

    /// Tells a screen reader `label`, the count or the pattern's error, if
    /// it's to be told ([`FindBar::announce`]) and is there: while the
    /// matches are counted it waits for the count.
    /// A count owed while another window is active is dropped: it would be
    /// said there.
    fn announce_label(&self, label: Option<&(SharedString, bool)>, cx: &mut App) {
        if !self.announce.get() {
            return;
        }
        if self.window.is_none() || cx.active_window() != self.window {
            self.announce.set(false);
            return;
        }
        if let Some((label, _)) = label {
            self.announce.set(false);
            fe_ui::announce(cx, label.clone(), fe_ui::Politeness::Polite);
        }
    }

    /// Flips an option and finds again from the anchor.
    fn toggle(
        &mut self,
        option: fn(&mut Self) -> &mut bool,
        editor: Option<&Entity<EditorView>>,
        cx: &mut App,
    ) {
        let flag = option(self);
        *flag = !*flag;
        self.search(editor, cx);
        self.announce_now();
    }

    /// Follows the pane's active editor, `editor`, while the bar is open:
    /// the editor it left has its find cleared, and the new one gets the
    /// pattern without its caret moving, its selection's start the anchor.
    pub(crate) fn follow(&mut self, editor: Option<&Entity<EditorView>>, cx: &mut App) {
        if !self.open {
            return;
        }
        let same = match (&self.editor, editor) {
            (Some(old), Some(new)) => old.entity_id() == new.entity_id(),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        self.set_editor(editor, cx);
        let Some(editor) = editor else {
            return;
        };
        let pattern = self.valid_pattern();
        editor.update(cx, |editor, cx| {
            editor.set_find(pattern, cx);
            // Selected before the bar came here, they're the user's.
            self.note_selections(editor);
            self.leave(editor, true);
        });
    }

    /// Where to find from in `editor`: the anchor, unless the selection has
    /// moved since the bar left it (a click, an edit, a reveal), and then
    /// the selection's start, which becomes the anchor.
    fn from(&mut self, editor: &EditorView) -> usize {
        let selection = editor.model().primary_selection().range;
        let left = self.left.as_ref().and_then(|(buffer, range)| {
            Some(resolve(*buffer, &range.start, editor)?..resolve(*buffer, &range.end, editor)?)
        });
        let anchor = self
            .anchor
            .filter(|_| left == Some(selection.clone()))
            .and_then(|(buffer, anchor)| resolve(buffer, &anchor, editor));
        anchor.unwrap_or_else(|| {
            self.anchor = Some(place(editor, selection.start));
            selection.start
        })
    }

    /// Notes `editor`'s selections as the bar leaves them, and with `anchor`
    /// makes the primary one's start the anchor.
    fn leave(&mut self, editor: &EditorView, anchor: bool) {
        let selection = editor.model().primary_selection().range;
        let snapshot = editor.model().snapshot();
        let range = snapshot.anchor_before(selection.start)..snapshot.anchor_after(selection.end);
        self.left = Some((snapshot.id(), range));
        let all = (editor.model().selections().into_iter())
            .map(|selection| {
                let range = selection.range;
                snapshot.anchor_before(range.start)..snapshot.anchor_after(range.end)
            })
            .collect();
        self.left_all = Some((snapshot.id(), all));
        if anchor {
            self.anchor = Some(place(editor, selection.start));
        }
    }

    /// Makes `editor` the one the bar's pattern is on, clearing the find of
    /// the one it was on before, if another, and dropping Find in Selection's
    /// selections, which were in that one's buffer.
    fn set_editor(&mut self, editor: Option<&Entity<EditorView>>, cx: &mut App) {
        let old = self.editor.take().and_then(|old| old.upgrade());
        let same = match (&old, editor) {
            (Some(old), Some(new)) => old.entity_id() == new.entity_id(),
            _ => false,
        };
        if !same {
            self.scope = None;
            self.selected = None;
            self.left_all = None;
        }
        if let Some(old) = old
            && !same
        {
            old.update(cx, |old, cx| old.set_find(None, cx));
        }
        self.editor = editor.map(Entity::downgrade);
    }

    /// What the bar shows beside the input, and whether it's an error:
    /// what the last replace all did, the pattern's error, or the count of
    /// `editor`'s matches, nothing while it's counted or the query is empty.
    pub(crate) fn label(&self, editor: Option<&EditorView>) -> Option<(SharedString, bool)> {
        if let Some((status, error)) = &self.status {
            return Some((status.resolve(), *error));
        }
        let pattern = self.pattern.as_ref()?;
        if let Err(error) = pattern {
            return Some((error.to_string().into(), true));
        }
        let count = editor?.find_count()?;
        Some((count_text(count.total, count.current, count.capped), false))
    }

    /// The toggles inside the find field, in order: what each runs, its
    /// icon, its accessible name and whether it's on.
    pub(crate) fn field_toggles(&self) -> [(FindOp, Icon, SharedString, bool); 3] {
        [
            (
                FindOp::ToggleCase,
                Icon::CaseSensitive,
                tr!("workbench-find-match-case"),
                self.match_case,
            ),
            (
                FindOp::ToggleWholeWord,
                Icon::WholeWord,
                tr!("workbench-find-whole-word"),
                self.whole_word,
            ),
            (
                FindOp::ToggleRegex,
                Icon::Regex,
                tr!("workbench-find-regex"),
                self.regex,
            ),
        ]
    }

    /// The Vim command that does what the bar does
    /// ([`fe_vim::search::replace::vim_command`]): with the replace row
    /// hidden, the `/` that finds the query's matches, under vim's
    /// 'ignorecase' and 'smartcase'; shown, the `:%s` that replaces them all
    /// as Replace All does. `None` with Find in Selection or, replacing,
    /// Preserve Case on, which Vim has no command for, or where Vim's
    /// syntax can't say the query or the replacement exactly.
    pub(crate) fn vim_command(&self, cx: &App) -> Option<String> {
        if self.scope.is_some() || (self.replacing && self.preserve_case) {
            return None;
        }
        let with = self.replacing.then(|| self.replacement(cx));
        let registers = fe_gpui::vim_registers(cx);
        fe_vim::search::replace::vim_command(&self.query(cx), with.as_ref(), |pattern| {
            registers.is_some_and(|registers| registers.ignores_case(pattern, false))
        })
    }

    /// The bar, under the pane's toolbar, while it's open.
    pub(crate) fn render(
        &self,
        editor: Option<&Entity<EditorView>>,
        ui: &UiTheme,
        cx: &mut Context<Pane>,
    ) -> Option<impl IntoElement + use<>> {
        if !self.open {
            return None;
        }
        let label = self.label(editor.map(|editor| editor.read(cx)));
        self.announce_label(label.as_ref(), cx);
        let toggles =
            (self.field_toggles().into_iter().enumerate()).map(|(ix, (op, icon, name, on))| {
                div()
                    .debug_selector(move || format!("find-toggle:{ix}"))
                    .child(
                        ToggleButton::icon_only(("find-option", ix), icon, name)
                            .on(on)
                            .on_click(cx.listener(move |pane, _, window, cx| {
                                pane.run_find(op, window, cx)
                            })),
                    )
            });
        let in_selection = div().debug_selector(|| "find-in-selection".into()).child(
            ToggleButton::icon_only(
                "find-in-selection",
                Icon::SquareDashedText,
                tr!("workbench-find-in-selection"),
            )
            .on(self.scope.is_some())
            .on_click(cx.listener(|pane, _, window, cx| {
                pane.run_find(FindOp::ToggleInSelection, window, cx)
            })),
        );
        let steps = [
            (
                FindOp::Previous,
                Icon::ChevronUp,
                "find-previous",
                tr!("workbench-find-previous"),
            ),
            (
                FindOp::Next,
                Icon::ChevronDown,
                "find-next",
                tr!("workbench-find-next"),
            ),
        ]
        .map(|(op, icon, id, name)| {
            div().debug_selector(move || id.into()).child(
                IconButton::new(id, icon, name, ui).on_click(
                    cx.listener(move |pane, _, window, cx| pane.run_find(op, window, cx)),
                ),
            )
        });
        let label = label.map(|(text, error)| {
            let color = if error { ui.error_text } else { ui.muted_text };
            div()
                .debug_selector(|| "find-count".into())
                .flex_none()
                // Clear of the field's focus ring.
                .pl_1()
                .max_w(px(240.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(color)
                .child(text)
        });
        let chevron = div()
            .flex_none()
            .w(px(CHEVRON_WIDTH))
            .pl_1()
            .debug_selector(|| "find-replace-toggle".into())
            // As the toggles do, the chevron keeps focus where it was.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .child(
                IconButton::new(
                    "find-replace-toggle",
                    if self.replacing {
                        Icon::ChevronDown
                    } else {
                        Icon::ChevronRight
                    },
                    tr!("workbench-find-toggle-replace"),
                    ui,
                )
                .selected(self.replacing)
                .on_click(cx.listener(|pane, _, window, cx| {
                    pane.run_find(FindOp::ToggleReplace, window, cx)
                })),
            );
        let vim_command = (fe_gpui::vim_enabled(cx))
            .then(|| self.vim_command(cx))
            .flatten()
            .map(|command| VimCommand {
                command,
                focus: self.vim_command_focus.clone(),
            });
        let find_row = div()
            .flex()
            .items_center()
            .gap_1()
            .py_1()
            .child(chevron)
            .child(field_slot().child(field(
                "find-field",
                &self.input,
                toggles.map(IntoElement::into_any_element).collect(),
                cx,
            )))
            .children(label)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_0p5()
                    // The pane takes focus on a click inside it unless
                    // something prevents the default; a button here keeps
                    // focus where it was, the input or the editor.
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .children(steps)
                    .child(in_selection),
            )
            .child(div().flex_1())
            .children(vim_command);
        let replace_row = self.replacing.then(|| {
            let preserve_case = div().debug_selector(|| "find-preserve-case".into()).child(
                ToggleButton::new(
                    "find-preserve-case",
                    verbatim_label("AB"),
                    tr!("workbench-find-preserve-case"),
                    ui,
                )
                .on(self.preserve_case)
                .on_click(cx.listener(|pane, _, _, cx| {
                    pane.update_find(|bar, _, _| bar.toggle_preserve_case(), cx);
                    cx.notify();
                })),
            );
            let buttons = [
                (
                    FindOp::ReplaceNext,
                    text!("workbench-find-replace"),
                    "find-replace",
                ),
                (
                    FindOp::ReplaceAll,
                    text!("workbench-find-replace-all"),
                    "find-replace-all",
                ),
            ]
            .map(|(op, label, id)| {
                div().debug_selector(move || id.into()).child(
                    Button::new(id, label).small().on_click(
                        cx.listener(move |pane, _, window, cx| pane.run_find(op, window, cx)),
                    ),
                )
            });
            div()
                .flex()
                .items_center()
                .gap_1()
                .pb_1()
                // Under the chevron, so the fields line up.
                .child(div().flex_none().w(px(CHEVRON_WIDTH)))
                .child(field_slot().key_context(REPLACE_KEY_CONTEXT).child(field(
                    "find-replace-field",
                    &self.replace_input,
                    vec![preserve_case.into_any_element()],
                    cx,
                )))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .gap_1()
                        // A replace keeps focus where it was too.
                        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                        .children(buttons),
                )
        });
        let bar = div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(on_input_command))
            // A tab dropped here goes where the marker in the tab bar says.
            .on_drop(cx.listener(Pane::on_drop))
            .flex_none()
            .flex()
            .flex_col()
            .pr_2()
            .w_full()
            .overflow_hidden()
            .bg(ui.panel_background)
            .border_b_1()
            .border_color(ui.border)
            .text_color(ui.text)
            .text_size(px(12.))
            .debug_selector(|| "pane-find-bar".into())
            .child(find_row)
            .children(replace_row);
        Some(SearchLandmark(bar))
    }
}

/// The bar as a screen reader's `Search` landmark, named "Find".
#[derive(IntoElement)]
struct SearchLandmark(Div);

impl RenderOnce for SearchLandmark {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let name = tr!("workbench-find-placeholder");
        node(self.0.id("pane-find-bar"), window, cx, Role::Search, name)
    }
}

/// `label`, a glyph rather than a word, which isn't translated.
pub(crate) fn verbatim_label(label: &'static str) -> SharedString {
    SharedString::new_static(label)
}

/// Where a find or replace field goes: [`FIELD_WIDTH`] wide, so the two
/// line up whatever follows each.
fn field_slot() -> gpui::Div {
    div()
        .flex_none()
        .flex_shrink(1.)
        .w(px(FIELD_WIDTH))
        .min_w(px(FIELD_MIN_WIDTH))
}

/// A find or replace field: `input` in the controls' bordered box with the
/// accent focus ring while it has focus, `toggles` inside it at its end.
pub(crate) fn field(
    id: &'static str,
    input: &Entity<TextInput>,
    toggles: Vec<AnyElement>,
    cx: &App,
) -> impl IntoElement {
    let colors = cx.theme().colors;
    div()
        .debug_selector(move || id.into())
        .relative()
        .flex()
        .items_center()
        .w_full()
        .rounded(radius::CONTROL)
        .border_1()
        .border_color(colors.border.control)
        .bg(colors.surface.panel)
        .child(div().flex_1().min_w_0().child(input.clone()))
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap_0p5()
                .pr_1()
                // A toggle keeps focus where it was, the input or the editor.
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .children(toggles),
        )
        .child(FieldRing {
            id,
            focus: input.focus_handle(cx),
        })
}

/// A field's focus ring, drawn while its input has focus: the controls'
/// 2 px of the accent, 1 px out ([`focus_ring`]).
#[derive(IntoElement)]
struct FieldRing {
    id: &'static str,
    focus: FocusHandle,
}

impl RenderOnce for FieldRing {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.id;
        match self.focus.is_focused(window) {
            true => focus_ring(radius::CONTROL, px(1.), &cx.theme().colors)
                .debug_selector(move || format!("{id}-focus-ring")),
            false => div(),
        }
    }
}

/// The Vim command, faint, in the code font, which a click copies, as
/// Enter or Space does while it has focus. A click leaves focus where it
/// was; the keyboard moves focus to it (it's a tab stop), and it shows the
/// focus ring while focused from the keyboard.
#[derive(IntoElement)]
struct VimCommand {
    command: String,
    focus: FocusHandle,
}

impl RenderOnce for VimCommand {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors;
        let family = (cx.try_global::<FontFamilies>().cloned().unwrap_or_default())
            .family(FamilySlot::Code, cx);
        let name = tr!("workbench-find-vim-command", command = self.command.clone());
        let tooltip = tr!("workbench-find-vim-command-tooltip");
        let copied = self.command.clone();
        let ring = self.focus.is_focused(window) && window.last_input_was_keyboard();
        let button = div().id("find-vim-command");
        node(button, window, cx, Role::Button, name)
            .debug_selector(|| "find-vim-command".into())
            .track_focus(&self.focus)
            .relative()
            .flex()
            .flex_shrink(1.)
            .min_w_0()
            .cursor_pointer()
            .tooltip(move |_, cx| {
                let tooltip =
                    Tooltip::new(tooltip.clone(), None::<SharedString>, &fe_ui::ui_theme(cx));
                cx.new(|_| tooltip).into()
            })
            // Copying leaves focus where it was: this runs before GPUI's
            // own focus on a mouse down, which a prevented default skips.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
            })
            .child(
                div()
                    .min_w_0()
                    .flex_shrink(1.)
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_family(family)
                    .text_color(colors.text.faint)
                    .child(self.command),
            )
            // Outside the clipped text, so it isn't cut off.
            .when(ring, |button| {
                button.child(
                    focus_ring(radius::CONTROL, px(0.), &colors)
                        .debug_selector(|| "find-vim-command-focus-ring".into()),
                )
            })
    }
}

/// The match the primary selection is in `editor`, or with vim on the one
/// starting at the caret, and its replacement by `with`, in the match's case
/// with `preserve_case`; none outside `scope`, for which nothing is searched
/// or expanded; `Err` if `deadline` passed before both were known.
fn current_match(
    editor: &EditorView,
    pattern: &Pattern,
    with: &Replacement,
    preserve_case: bool,
    scope: Option<&[Range<usize>]>,
    deadline: &mut Deadline,
) -> Result<Option<Edit>, Interrupted> {
    let text = editor.model().text();
    let inside = |range: &Range<usize>| {
        scope.is_none_or(|scope| EditorView::in_find_scope(text, scope, range))
    };
    let selection = editor.model().primary_selection().range;
    if !inside(&selection) {
        return Ok(None);
    }
    let range = match editor.vim_mode() {
        Some(_) if selection.is_empty() => {
            // Only a match starting at the caret is wanted, so the search
            // stops past it: rejecting candidates later on the row (not
            // whole words, say) could take the rest of a long row.
            let row = text.offset_to_point(selection.start).row;
            let window = selection.start..(selection.start + 1).min(row_end(text, row));
            match first_reaching(text, row, window, pattern, deadline)? {
                Some(found) if found.start == selection.start => found,
                _ => return Ok(None),
            }
        }
        _ => selection,
    };
    // With vim on it's the match at the caret, which may run out of the scope.
    if !inside(&range) {
        return Ok(None);
    }
    let replacement = pattern.replacement_at(text, range.clone(), with);
    // Its own search has its own deadline, which can't pass before this one.
    if deadline.expired() {
        return Err(Interrupted);
    }
    let replacement = replacement.map(|replacement| match preserve_case {
        true => preserve_case::apply(&text.slice_to_string(range.clone()), &replacement),
        false => replacement,
    });
    Ok(replacement.map(|replacement| (range, normalize_newlines(replacement))))
}

/// `text` with its line breaks made `\n`, as [`fe_buffer::Buffer::edit`]
/// makes them, so the text a replace is planned against is the text it
/// makes: a replacement can hold a lone `\r` pasted into its input.
/// What a count with the bar closed is of: the editor Find Next or
/// Previous stepped in, in which window, its text then and the match it
/// left selected (or the caret at, with vim on).
struct Source {
    editor: WeakEntity<EditorView>,
    window: Option<AnyWindowHandle>,
    version: fe_buffer::Version,
    at: Range<usize>,
}

impl Source {
    /// Whether the count is still of what the user is on: the same text,
    /// the match where the step left it, and the editor focused in the
    /// active window. An edit, a move, a tab or window switch says no.
    fn stands(&self, cx: &mut App) -> bool {
        let Some(editor) = self.editor.upgrade() else {
            return false;
        };
        let view = editor.read(cx);
        let model = view.model();
        let selection = model.primary_selection();
        let at = match view.vim_mode() {
            Some(_) => selection.head()..selection.head(),
            None => selection.range,
        };
        if model.snapshot().version() != self.version || at != self.at {
            return false;
        }
        let Some(window) = self
            .window
            .filter(|window| cx.active_window() == Some(*window))
        else {
            return false;
        };
        (window.update(cx, |_, window, cx| {
            editor.focus_handle(cx).contains_focused(window, cx)
        }))
        .unwrap_or(false)
    }
}

/// The count as the bar shows it: `2 of 5`, `No results`, or `5`
/// matches with none selected, each with more past a cap of `total`.
fn count_text(total: usize, current: Option<usize>, capped: bool) -> SharedString {
    match (current, capped) {
        _ if total == 0 => tr!("workbench-find-no-results"),
        (Some(current), false) => tr!("workbench-find-count", current = current, total = total),
        (Some(current), true) => {
            tr!(
                "workbench-find-count-capped",
                current = current,
                total = total
            )
        }
        (None, false) => tr!("workbench-find-count-unknown", total = total),
        (None, true) => tr!("workbench-find-count-unknown-capped", total = total),
    }
}

/// The most matches a count with the bar closed counts, as the editor's
/// does.
const MAX_COUNTED: usize = 10_000;

/// `pattern`'s matches in `text`, by start, the first [`MAX_COUNTED`], and
/// whether there are more.
fn count_matches(text: &Text, pattern: &Pattern) -> (Vec<Range<usize>>, bool) {
    let rows = 0..text.row_count();
    let mut ranges = fe_vim::search::matches_in_until(
        text,
        rows,
        pattern,
        MAX_COUNTED + 1,
        &mut Deadline::never(),
    );
    let capped = ranges.len() > MAX_COUNTED;
    ranges.truncate(MAX_COUNTED);
    (ranges, capped)
}

fn normalize_newlines(text: String) -> String {
    match text.contains('\r') {
        true => text.replace("\r\n", "\n").replace('\r', "\n"),
        false => text,
    }
}

/// The first match at or after `from` (an empty one at `from` only with
/// `empty_at_from`), row by row to the end and round from the start: of
/// those [`replace_all`] replaces and [`Pattern::replacement_at`] takes,
/// each row's stepped through only as far as the one wanted, under
/// `deadline` for every row. With a `scope`, only those inside it. `Err` if
/// it passed before that was known.
fn next_match(
    text: &Text,
    pattern: &Pattern,
    from: usize,
    empty_at_from: bool,
    scope: Option<&[Range<usize>]>,
    deadline: &mut Deadline,
) -> Result<Option<Range<usize>>, Interrupted> {
    let first = text.offset_to_point(from).row;
    let end = row_end(text, first);
    let wanted = |found: &Range<usize>| {
        found.start > from || (found.start == from && (empty_at_from || !found.is_empty()))
    };
    if let Some(scope) = scope {
        return EditorView::next_in_find_scope(text, pattern, scope, from, wanted, deadline);
    }
    // The first match reaching `from`'s window, unless it's one from before
    // `from` reaching past it, or the empty one at it: then the one after.
    let mut found = first_reaching(text, first, from..end, pattern, deadline)?;
    if let Some(unwanted) = found.clone().filter(|found| !wanted(found)) {
        let past = match unwanted.start < from {
            true => unwanted.end,
            false => from + 1,
        };
        found = match past <= end {
            true => first_reaching(text, first, past..end, pattern, deadline)?.filter(wanted),
            false => None,
        };
    }
    if found.is_some() {
        return Ok(found);
    }
    let rows = (first + 1..text.row_count()).chain(0..=first);
    for row in rows {
        let window = text.row_start(row)..row_end(text, row);
        if let Some(found) = first_reaching(text, row, window, pattern, deadline)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// `scope`, sorted and apart, in the text as replacing `replaced`, which is
/// inside one of its ranges, with `new_len` bytes leaves it, which is where
/// its anchors will go, a range that leaves empty gone as from
/// [`EditorView::find_scope`].
fn scope_after(
    scope: Vec<Range<usize>>,
    replaced: &Range<usize>,
    new_len: usize,
) -> Vec<Range<usize>> {
    let shift = |offset: usize| offset - replaced.len() + new_len;
    (scope.into_iter())
        .map(|range| match range {
            range if range.end < replaced.start => range,
            range if range.start > replaced.end => shift(range.start)..shift(range.end),
            range => range.start..shift(range.end),
        })
        .filter(|range| !range.is_empty())
        .collect()
}

/// The first match on `row` that reaches `window`, of those [`replace_all`]
/// replaces, stepping through the row's matches no further; `Err` if
/// `deadline` passed, during the search or by its end.
fn first_reaching(
    text: &Text,
    row: u32,
    window: Range<usize>,
    pattern: &Pattern,
    deadline: &mut Deadline,
) -> Result<Option<Range<usize>>, Interrupted> {
    let found = matches_reaching_until(text, row, window, pattern, 1, deadline);
    match deadline.expired() {
        true => Err(Interrupted),
        false => Ok(found.into_iter().next()),
    }
}

/// The offset at the end of `row`, before its line break.
fn row_end(text: &Text, row: u32) -> usize {
    text.row_start(row) + text.row_len(row) as usize
}

/// Selects `found` and reveals it; with vim on, puts the caret at its
/// start instead, as [`EditorView::find_next`] does.
fn select_match(editor: &mut EditorView, found: Range<usize>, cx: &mut Context<EditorView>) {
    let range = match editor.vim_mode() {
        Some(_) => found.start..found.start,
        None => found,
    };
    editor.select_and_reveal(range, cx);
}

/// Makes `edits`, sorted and apart, in `editor` as one transaction, the
/// user's, of its own even if another of the user's is open (vim's insert
/// session, here or in another view of the buffer), so one undo takes them
/// back and only them; and puts the caret after the last: with vim in
/// normal mode, on the last grapheme of its row if that's past it. An open
/// vim insert or replace session stays open, its typing going on at the
/// caret: nothing ending it does (`Esc`'s copies or indentation taken out)
/// happens, so the text is just what the edits make.
fn apply(editor: &mut EditorView, edits: Vec<Edit>, cx: &mut Context<EditorView>) {
    let normal = !matches!(editor.vim_mode(), None | Some("insert" | "replace"));
    let model = editor.model_mut();
    model.sync_display();
    let last = edits.last().map_or(0, |(range, _)| range.end);
    let after = model.snapshot().anchor_after(last);
    let mut buffer = model.buffer_mut();
    let version = buffer.version();
    buffer.with_transaction_aside(|buffer| buffer.edit(edits, Origin::User));
    buffer.note_edits_since(version);
    drop(buffer);
    let end = model.snapshot().resolve(&after);
    let row = model.text().offset_to_point(end).row;
    let row_start = model.text().row_start(row);
    let caret = match normal && end > row_start && end == row_end(model.text(), row) {
        // Back onto the row's last grapheme, as the model steps.
        true => {
            model.set_cursor(end);
            model.move_cursors(Movement::Left, false);
            model.primary_selection().head()
        }
        false => end,
    };
    editor.select_and_reveal(caret..caret, cx);
}

/// A place at `offset` in `editor`'s buffer that stays before text
/// inserted there.
fn place(editor: &EditorView, offset: usize) -> (BufferId, Anchor) {
    let snapshot = editor.model().snapshot();
    (snapshot.id(), snapshot.anchor_before(offset))
}

/// Where `anchor`, in `buffer`, is now in `editor`'s text; `None` if that's
/// another buffer, or its version can't be resolved there (its log
/// compacted past it).
fn resolve(buffer: BufferId, anchor: &Anchor, editor: &EditorView) -> Option<usize> {
    let snapshot = editor.model().snapshot();
    let versions = snapshot.oldest_version()..=snapshot.version();
    (buffer == snapshot.id() && versions.contains(&anchor.version()))
        .then(|| snapshot.resolve(anchor))
}

/// A typed command, as its action, the find commands among them; the
/// focused input's editing commands; others go on.
fn on_input_command(
    pane: &mut Pane,
    run: &RunCommand,
    window: &mut Window,
    cx: &mut Context<Pane>,
) {
    if kiln::commands::dispatch_typed(run, window, cx) {
        return;
    }
    let Some(edit) = picker::op(run, cx).and_then(PaletteOp::edit) else {
        return cx.propagate();
    };
    pane.update_find(
        |bar, _, cx| bar.focused_input(window, cx).clone().update(cx, edit),
        cx,
    );
}

#[cfg(test)]
#[path = "find_replace_tests.rs"]
mod replace_tests;

#[cfg(test)]
#[path = "find_preserve_case_tests.rs"]
mod preserve_case_tests;

#[cfg(test)]
#[path = "find_in_selection_tests.rs"]
mod in_selection_tests;

/// The Vim command from the keyboard: it's a tab stop, and Enter or Space
/// copies it while it has focus (T557 F6).
#[cfg(test)]
mod vim_command_keys {
    use fe_gpui::{set_vim_enabled, set_vim_search_case};
    use gpui::{Focusable as _, TestAppContext, VisualTestContext};

    use crate::tests::find_bar::{draw, input_focused, opened, pane, run};

    fn copied(cx: &mut VisualTestContext) -> Option<String> {
        cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
    }

    /// Presses and releases `key`: GPUI clicks a focused element when
    /// Enter or Space is released.
    fn press(key: &str, cx: &mut VisualTestContext) {
        cx.simulate_keystrokes(key);
        cx.simulate_event(gpui::KeyUpEvent {
            keystroke: gpui::Keystroke::parse(key).unwrap(),
        });
        draw(cx);
    }

    #[gpui::test]
    fn the_keyboard_reaches_and_presses_the_vim_command(cx: &mut TestAppContext) {
        let (_dir, workbench, cx) = opened(cx, &[("a.txt", "dirty\n")]);
        let pane = pane(&workbench, cx);
        cx.update(|_, cx| {
            set_vim_enabled(cx, true);
            set_vim_search_case(cx, true, true);
        });
        run("find.show", cx);
        cx.simulate_input("dirty");
        draw(cx);
        assert!(input_focused(&pane, cx));
        let (input, command) = pane.read_with(cx, |pane, cx| {
            let bar = pane.find_bar().unwrap();
            (bar.input().focus_handle(cx), bar.vim_command_focus.clone())
        });

        // Tabbing on from the query comes to it.
        let mut reached = false;
        for _ in 0..16 {
            cx.update(|window, cx| window.focus_next(cx));
            draw(cx);
            if cx.update(|window, _| command.is_focused(window)) {
                reached = true;
                break;
            }
        }
        assert!(reached, "the Vim command is in the tab order");
        assert!(
            cx.debug_bounds("find-vim-command-focus-ring").is_some(),
            "focused from the keyboard, it shows the ring"
        );

        press("enter", cx);
        assert_eq!(copied(cx).as_deref(), Some("/dirty"), "enter copies it");
        cx.update(|_, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string(String::new())));
        press("space", cx);
        assert_eq!(copied(cx).as_deref(), Some("/dirty"), "space copies it");
        assert!(
            cx.update(|window, _| command.is_focused(window)),
            "focus stays on it"
        );

        // A click still leaves focus where it was, with no ring.
        cx.update(|window, cx| window.focus(&input, cx));
        draw(cx);
        let at = cx.debug_bounds("find-vim-command").unwrap().center();
        cx.simulate_click(at, gpui::Modifiers::none());
        draw(cx);
        assert!(input_focused(&pane, cx));
        assert!(cx.debug_bounds("find-vim-command-focus-ring").is_none());
    }
}
