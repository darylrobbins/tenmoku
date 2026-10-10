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
//! The commands are in [`registry`]; the pane runs them on its root
//! ([`crate::Pane::run_find`]). The bar's element sets [`KEY_CONTEXT`] and
//! its input `text_input`, and neither carries `vim.mode`, so with vim on
//! keys typed there only type.

use std::{ops::Range, rc::Rc};

use fe_buffer::{Anchor, BufferId};
use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_ui::{TextInput, TextInputEvent, ToggleButton, UiTheme};
use fe_vim::search::{FindQuery, Pattern, PatternError};
use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, Global, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, SharedString, Styled as _, Subscription,
    WeakEntity, Window, div, px,
};

use crate::{
    Pane,
    picker::{self, PaletteOp},
};

/// The key context the find bar's element sets.
pub(crate) const KEY_CONTEXT: &str = "find_bar";

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
}

/// A pane's find bar: see the module docs.
pub(crate) struct FindBar {
    open: bool,
    input: Entity<TextInput>,
    match_case: bool,
    whole_word: bool,
    regex: bool,
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
        let changed = cx.subscribe(&input, |pane, _, event, cx| {
            if *event == TextInputEvent::Changed {
                pane.update_find(|bar, editor, cx| bar.search(editor, cx), cx);
                cx.notify();
            }
        });
        Self {
            open: false,
            input,
            match_case: false,
            whole_word: false,
            regex: false,
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
        match op {
            FindOp::Show => self.show(editor, window, cx),
            FindOp::Next => self.step(true, editor, cx),
            FindOp::Previous => self.step(false, editor, cx),
            FindOp::Close => {
                self.open = false;
                self.set_editor(None, cx);
            }
            FindOp::ToggleCase => self.toggle(|bar| &mut bar.match_case, editor, cx),
            FindOp::ToggleWholeWord => self.toggle(|bar| &mut bar.whole_word, editor, cx),
            FindOp::ToggleRegex => self.toggle(|bar| &mut bar.regex, editor, cx),
        }
    }

    /// Opens the bar, or keeps it open, with focus in its input. A
    /// non-empty selection within one row replaces the query, and the
    /// selection's start is the anchor.
    fn show(&mut self, editor: Option<&Entity<EditorView>>, window: &mut Window, cx: &mut App) {
        self.open = true;
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
        window.focus(&self.input.focus_handle(cx), cx);
    }

    /// Builds the pattern, sets it on `editor` and selects the first match
    /// from the anchor. An empty or invalid query clears the editor's find.
    fn search(&mut self, editor: Option<&Entity<EditorView>>, cx: &mut App) {
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
    /// the pattern's error, or the count of `editor`'s matches, nothing
    /// while it's counted or the query is empty.
    pub(crate) fn label(&self, editor: Option<&EditorView>) -> Option<(SharedString, bool)> {
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
        let bar = div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(on_input_command))
            // A tab dropped here goes where the marker in the tab bar says.
            .on_drop(cx.listener(Pane::on_drop))
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .pr_2()
            .w_full()
            .overflow_hidden()
            .bg(ui.panel_background)
            .border_b_1()
            .border_color(ui.border)
            .text_color(ui.text)
            .text_size(px(12.))
            .debug_selector(|| "pane-find-bar".into())
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
        Some(bar)
    }
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

/// The input's editing commands; others go on to the pane.
fn on_input_command(pane: &mut Pane, run: &RunCommand, _: &mut Window, cx: &mut Context<Pane>) {
    let edit: fn(&mut TextInput, &mut Context<TextInput>) = match picker::op(run, cx) {
        Some(PaletteOp::Backspace) => TextInput::backspace,
        Some(PaletteOp::DeleteWordLeft) => TextInput::delete_word_left,
        Some(PaletteOp::Clear) => TextInput::clear,
        Some(PaletteOp::Paste) => TextInput::paste,
        _ => return cx.propagate(),
    };
    pane.update_find(|bar, _, cx| bar.input.update(cx, edit), cx);
}

/// The [`CommandTarget`] of find commands: the op to do.
pub(crate) struct FindCommands;

impl CommandTarget for FindCommands {
    type Tx = ();
    type Target<'a> = Option<FindOp>;
}

struct Commands(Rc<Registry<FindCommands>>);

impl Global for Commands {}

pub(crate) fn registry() -> Registry<FindCommands> {
    use FindOp::*;
    let editor = "editor";
    let either = "editor || find_bar";
    let commands = [
        ("find.show", "Find", editor, Show),
        ("find.next", "Find Next", either, Next),
        ("find.previous", "Find Previous", either, Previous),
        ("find.close", "Close Find", KEY_CONTEXT, Close),
        ("find.toggle_case", "Match Case", KEY_CONTEXT, ToggleCase),
        (
            "find.toggle_whole_word",
            "Match Whole Word",
            KEY_CONTEXT,
            ToggleWholeWord,
        ),
        (
            "find.toggle_regex",
            "Use Regular Expression",
            KEY_CONTEXT,
            ToggleRegex,
        ),
    ];
    let mut registry = Registry::new();
    for (id, title, context, op) in commands {
        registry
            .register(id, title, move |target: &mut Option<FindOp>, _: &Actor| {
                *target = Some(op);
                Ok(Dispatched::new(Effect::Notify))
            })
            .expect("find command IDs are valid and distinct")
            .context(ContextPredicate::parse(context).expect("find contexts parse"));
    }
    registry
}

/// The op `run` asks for, if it's a find command in a context where it
/// applies.
pub(crate) fn op(run: &RunCommand, cx: &mut App) -> Option<FindOp> {
    if !cx.has_global::<Commands>() {
        cx.set_global(Commands(Rc::new(registry())));
    }
    let registry = cx.global::<Commands>().0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}
