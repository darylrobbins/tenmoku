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
//! The commands are the pane's typed commands ([`crate::find_commands`]),
//! which its root runs ([`crate::Pane::run_find`]). The bar's element sets
//! [`KEY_CONTEXT`] and its inputs `text_input`, the replace input
//! [`REPLACE_KEY_CONTEXT`] too, and none carries `vim.mode`, so with vim on keys typed there only type.

use std::ops::Range;

use fe_buffer::{Anchor, BufferId, Origin, Text};
use fe_editor::Movement;
use fe_gpui::{EditorView, commands::RunCommand};
use fe_ui::{TextInput, TextInputEvent, ToggleButton, UiTheme};
use fe_vim::search::{
    Deadline, FindQuery, Interrupted, Pattern, PatternError, matches_reaching_until,
    replace::{Replacement, replace_all},
};
use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, SharedString, Styled as _, Subscription, WeakEntity, Window,
    div, px,
};

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
    ToggleReplace,
    ShowReplace,
    ReplaceNext,
    ReplaceAll,
}

/// A pane's find bar: see the module docs.
pub(crate) struct FindBar {
    open: bool,
    input: Entity<TextInput>,
    /// Whether the replace row shows.
    replacing: bool,
    replace_input: Entity<TextInput>,
    /// What the label says in place of the count after a replace all, and
    /// whether it's an error, until the next find command or query change.
    status: Option<(SharedString, bool)>,
    match_case: bool,
    whole_word: bool,
    regex: bool,
    /// Whether a replacement takes the case of the match it replaces. It
    /// changes no match, so toggling it doesn't search again.
    preserve_case: bool,
    /// What the query makes; `None` while it's empty.
    pattern: Option<Result<Pattern, PatternError>>,
    /// Where a change to the query or an option finds from, in the buffer
    /// of the editor last found in, so edits move it: see [`FindBar::from`].
    anchor: Option<(BufferId, Anchor)>,
    /// The primary selection as the bar last left it, in that buffer: one
    /// that has moved since is the user's, and its start the anchor.
    left: Option<(BufferId, Range<Anchor>)>,
    /// The editor the bar set its pattern on, while it's open.
    editor: Option<WeakEntity<EditorView>>,
    _input: Subscription,
}

impl FindBar {
    pub(crate) fn new(ui: &UiTheme, cx: &mut Context<Pane>) -> Self {
        let input = cx.new(|cx| TextInput::new("Find", ui, cx));
        let replace_input = cx.new(|cx| TextInput::new("Replace", ui, cx));
        let changed = cx.subscribe(&input, |pane, _, event, cx| {
            if *event == TextInputEvent::Changed {
                pane.update_find(|bar, editor, cx| bar.search(editor, cx), cx);
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
            pattern: None,
            anchor: None,
            left: None,
            editor: None,
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
                self.set_editor(None, cx);
            }
            FindOp::ToggleCase => self.toggle(|bar| &mut bar.match_case, editor, cx),
            FindOp::ToggleWholeWord => self.toggle(|bar| &mut bar.whole_word, editor, cx),
            FindOp::ToggleRegex => self.toggle(|bar| &mut bar.regex, editor, cx),
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
        self.replacing |= replace;
        if let Some(editor) = editor {
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
        self.set_editor(editor, cx);
        self.search(editor, cx);
        let input = if replace {
            &self.replace_input
        } else {
            &self.input
        };
        window.focus(&input.focus_handle(cx), cx);
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
    /// replace session stays open ([`apply`]).
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
            let step = match self.plan_next(editor, &pattern, &with, self.preserve_case) {
                Ok(step) => step,
                Err(Interrupted) => {
                    self.status = Some(("Search took too long".into(), true));
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
        let current = match editor.is_read_only() {
            true => None,
            false => current_match(editor, pattern, with, preserve_case, &mut deadline)?,
        };
        let text = editor.model().text();
        let Some((range, new)) = current else {
            let from = self.from(editor);
            let next = next_match(editor.model().text(), pattern, from, true, &mut deadline)?;
            return Ok(Step::Select(next));
        };
        let mut replaced = text.clone();
        replaced.replace(range.clone(), &new);
        let end = range.start + new.len();
        let next = next_match(&replaced, pattern, end, !range.is_empty(), &mut deadline)?;
        Ok(Step::Replace((range, new), next))
    }

    /// Replaces every match in one edit, the caret after the last, and says
    /// how many in the label, or that the search took too long, replacing
    /// none and changing nothing. An open vim insert or replace session
    /// stays open ([`apply`]).
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
            let text = editor.model().text();
            let edits = match replace_all(text, &pattern, &with) {
                Ok(edits) if self.preserve_case => (edits.into_iter())
                    .map(|(range, new)| {
                        let new = preserve_case::apply(&text.slice_to_string(range.clone()), &new);
                        (range, new)
                    })
                    .collect(),
                Ok(edits) => edits,
                // Nothing changes, vim's mode and selection neither.
                Err(Interrupted) => {
                    self.status = Some(("Search took too long".into(), true));
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
            self.status = Some((format!("Replaced {count}").into(), false));
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
            let found = pattern.is_some();
            editor.set_find(pattern, cx);
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
        editor.update(cx, |editor, cx| {
            let transient = pattern.is_some();
            if transient {
                editor.set_find(pattern, cx);
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
        });
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

    /// Notes `editor`'s primary selection as the bar leaves it, and with
    /// `anchor` makes its start the anchor.
    fn leave(&mut self, editor: &EditorView, anchor: bool) {
        let selection = editor.model().primary_selection().range;
        let snapshot = editor.model().snapshot();
        let range = snapshot.anchor_before(selection.start)..snapshot.anchor_after(selection.end);
        self.left = Some((snapshot.id(), range));
        if anchor {
            self.anchor = Some(place(editor, selection.start));
        }
    }

    /// Makes `editor` the one the bar's pattern is on, clearing the find of
    /// the one it was on before, if another.
    fn set_editor(&mut self, editor: Option<&Entity<EditorView>>, cx: &mut App) {
        let old = self.editor.take().and_then(|old| old.upgrade());
        if let Some(old) = old
            && Some(old.entity_id()) != editor.map(Entity::entity_id)
        {
            old.update(cx, |old, cx| old.set_find(None, cx));
        }
        self.editor = editor.map(Entity::downgrade);
    }

    /// What the bar shows beside the input, and whether it's an error:
    /// what the last replace all did, the pattern's error, or the count of
    /// `editor`'s matches, nothing while it's counted or the query is empty.
    pub(crate) fn label(&self, editor: Option<&EditorView>) -> Option<(SharedString, bool)> {
        if let Some(status) = &self.status {
            return Some(status.clone());
        }
        let pattern = self.pattern.as_ref()?;
        if let Err(error) = pattern {
            return Some((error.to_string().into(), true));
        }
        let count = editor?.find_count()?;
        let text = if count.total == 0 {
            "No results".to_string()
        } else {
            let current = count
                .current
                .map_or_else(|| "?".to_string(), |current| current.to_string());
            let more = if count.capped { "+" } else { "" };
            format!("{current} of {}{more}", count.total)
        };
        Some((text.into(), false))
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
        let toggles = [
            (FindOp::ToggleCase, "Aa", self.match_case),
            (FindOp::ToggleWholeWord, "ab", self.whole_word),
            (FindOp::ToggleRegex, ".*", self.regex),
        ]
        .into_iter()
        .enumerate()
        .map(|(ix, (op, text, on))| {
            div()
                .debug_selector(move || format!("find-toggle:{ix}"))
                .child(
                    ToggleButton::new(("find-option", ix), text, ui)
                        .on(on)
                        .on_click(
                            cx.listener(move |pane, _, window, cx| pane.run_find(op, window, cx)),
                        ),
                )
        });
        let label = label.map(|(text, error)| {
            let color = if error { ui.error_text } else { ui.muted_text };
            div()
                .flex_none()
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
                ToggleButton::new(
                    "find-replace-toggle",
                    if self.replacing { "▾" } else { "▸" },
                    ui,
                )
                .on(self.replacing)
                .on_click(cx.listener(|pane, _, window, cx| {
                    pane.run_find(FindOp::ToggleReplace, window, cx)
                })),
            );
        let find_row = div()
            .flex()
            .items_center()
            .gap_1()
            .child(chevron)
            .child(div().flex_1().min_w_0().child(self.input.clone()))
            .children(label)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap_0p5()
                    // The pane takes focus on a click inside it unless
                    // something prevents the default; a toggle keeps focus
                    // where it was, the input or the editor.
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .children(toggles),
            );
        let replace_row = self.replacing.then(|| {
            let preserve_case = div().debug_selector(|| "find-preserve-case".into()).child(
                ToggleButton::new("find-preserve-case", "AB", ui)
                    .on(self.preserve_case)
                    .on_click(cx.listener(|pane, _, _, cx| {
                        pane.update_find(|bar, _, _| bar.toggle_preserve_case(), cx);
                        cx.notify();
                    })),
            );
            let buttons = [
                (FindOp::ReplaceNext, "Replace", "find-replace"),
                (FindOp::ReplaceAll, "Replace All", "find-replace-all"),
            ]
            .map(|(op, text, id)| {
                div().debug_selector(move || id.into()).child(
                    ToggleButton::new(id, text, ui).on_click(
                        cx.listener(move |pane, _, window, cx| pane.run_find(op, window, cx)),
                    ),
                )
            });
            div()
                .flex()
                .items_center()
                .gap_1()
                // Under the chevron, so the inputs line up.
                .child(div().flex_none().w(px(CHEVRON_WIDTH)))
                .child(
                    div()
                        .key_context(REPLACE_KEY_CONTEXT)
                        .flex_1()
                        .min_w_0()
                        .child(self.replace_input.clone()),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .gap_0p5()
                        // A replace keeps focus where it was too.
                        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                        .child(preserve_case)
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
        Some(bar)
    }
}

/// The match the primary selection is in `editor`, or with vim on the one
/// starting at the caret, and its replacement by `with`, in the match's case
/// with `preserve_case`; `Err` if `deadline` passed before both were known.
fn current_match(
    editor: &EditorView,
    pattern: &Pattern,
    with: &Replacement,
    preserve_case: bool,
    deadline: &mut Deadline,
) -> Result<Option<Edit>, Interrupted> {
    let text = editor.model().text();
    let selection = editor.model().primary_selection().range;
    let range = match editor.vim_mode() {
        Some(_) if selection.is_empty() => {
            let row = text.offset_to_point(selection.start).row;
            let window = selection.start..row_end(text, row);
            match first_reaching(text, row, window, pattern, deadline)? {
                Some(found) if found.start == selection.start => found,
                _ => return Ok(None),
            }
        }
        _ => selection,
    };
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
/// `deadline` for every row. `Err` if it passed before that was known.
fn next_match(
    text: &Text,
    pattern: &Pattern,
    from: usize,
    empty_at_from: bool,
    deadline: &mut Deadline,
) -> Result<Option<Range<usize>>, Interrupted> {
    let first = text.offset_to_point(from).row;
    let end = row_end(text, first);
    let wanted = |found: &Range<usize>| {
        found.start > from || (found.start == from && (empty_at_from || !found.is_empty()))
    };
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
