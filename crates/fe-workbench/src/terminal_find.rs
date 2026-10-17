//! The terminal panel's find row: a row above the active terminal, under the
//! tab strip, that finds in its output and scrollback, with a query, match
//! case and regex toggles, and the query's error or "No results".
//!
//! `terminal.find` opens it with focus in its input. Each change to the
//! query or a toggle sets the query on the active [`TerminalView`]
//! ([`TerminalView::set_find`]), which highlights its matches, then selects
//! the newest match, found up from the cursor. `find.next` steps up, toward
//! older output, and `find.previous` down, as VS Code's terminal find does;
//! both wrap. `find.close` clears the query on the terminal, closes the row
//! and focuses the terminal. While the row is open the query follows the
//! active terminal.
//!
//! The row's element sets [`find_bar::KEY_CONTEXT`], so the pane's find bar
//! keys apply; it runs the find commands itself
//! ([`find_commands::handle_in`]) and stops them there.

use fe_gpui::commands::RunCommand;
use fe_terminal_view::{QueryError, TerminalQuery, TerminalView};
use fe_ui::{TextInput, TextInputEvent, ToggleButton, UiTheme};
use gpui::{
    App, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    MouseButton, ParentElement as _, SharedString, Styled as _, Subscription, WeakEntity, Window,
    div, px,
};

use crate::{
    TerminalPanel,
    find_bar::{self, FindOp},
    find_commands,
    picker::{self, PaletteOp},
};

/// The panel's find row: see the module docs.
pub(crate) struct TerminalFind {
    open: bool,
    input: Entity<TextInput>,
    match_case: bool,
    regex: bool,
    /// Why the query doesn't compile, while it doesn't.
    error: Option<QueryError>,
    /// The terminal the row set its query on, while it's open.
    view: Option<WeakEntity<TerminalView>>,
    /// The query was set on `view` before its session started, so it
    /// wasn't checked: see [`TerminalFind::settle`].
    unchecked: bool,
    _input: Subscription,
}

impl TerminalFind {
    pub(crate) fn new(ui: &UiTheme, cx: &mut Context<TerminalPanel>) -> Self {
        let input = cx.new(|cx| TextInput::new("Find", ui, cx));
        let changed = cx.subscribe(&input, |panel, _, event, cx| {
            if *event == TextInputEvent::Changed {
                panel.update_find(|find, view, cx| find.search(view, cx), cx);
                cx.notify();
            }
        });
        Self {
            open: false,
            input,
            match_case: false,
            regex: false,
            error: None,
            view: None,
            unchecked: false,
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

    /// The query the input and toggles make: match case off is smart case.
    pub(crate) fn query(&self, cx: &App) -> TerminalQuery {
        TerminalQuery {
            text: self.input.read(cx).text().to_owned(),
            regex: self.regex,
            case_sensitive: self.match_case.then_some(true),
        }
    }

    /// Does `op` in `view`, the panel's active terminal, if any.
    /// `find.toggle_whole_word` and the replace commands do nothing here.
    pub(crate) fn run(
        &mut self,
        op: FindOp,
        view: Option<&Entity<TerminalView>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        match op {
            FindOp::Show => self.show(view, window, cx),
            FindOp::Next => self.step(false, view, cx),
            FindOp::Previous => self.step(true, view, cx),
            FindOp::Close => {
                self.open = false;
                self.unchecked = false;
                self.set_view(None, cx);
            }
            FindOp::ToggleCase => {
                self.match_case = !self.match_case;
                self.search(view, cx);
            }
            FindOp::ToggleRegex => {
                self.regex = !self.regex;
                self.search(view, cx);
            }
            FindOp::ToggleWholeWord
            | FindOp::ToggleReplace
            | FindOp::ShowReplace
            | FindOp::ReplaceNext
            | FindOp::ReplaceAll => {}
        }
    }

    /// Opens the row, or keeps it open, with focus in its input. A
    /// non-empty selection within one row on screen replaces the query.
    fn show(&mut self, view: Option<&Entity<TerminalView>>, window: &mut Window, cx: &mut App) {
        self.open = true;
        let selected = view.and_then(|view| {
            let terminal = view.read(cx).terminal()?;
            // A span per row: a line soft-wrapped onto the next is two.
            let one_row = terminal.snapshot().selection.len() == 1;
            terminal.selected_text().filter(|_| one_row)
        });
        if let Some(selected) = selected {
            self.input
                .update(cx, |input, cx| input.set_text(selected, cx));
        }
        self.set_view(view, cx);
        self.search(view, cx);
        window.focus(&self.input.focus_handle(cx), cx);
    }

    /// Sets the query on `view` and selects its newest match, found up from
    /// the cursor. A query that doesn't compile clears the terminal's find.
    fn search(&mut self, view: Option<&Entity<TerminalView>>, cx: &mut App) {
        self.error = None;
        self.unchecked = false;
        if !self.open {
            return;
        }
        let Some(view) = view else {
            return;
        };
        let query = self.query(cx);
        view.update(cx, |view, cx| {
            let empty = query.text.is_empty();
            self.unchecked = view.terminal().is_none() && !empty;
            if let Err(error) = view.set_find(Some(query), cx) {
                self.error = Some(error);
                view.set_find(None, cx).ok();
                return;
            }
            if empty {
                return;
            }
            if let Some(terminal) = view.terminal() {
                terminal.clear_selection();
            }
            if let Err(error) = view.find_next(false, cx) {
                self.error = Some(error);
                view.set_find(None, cx).ok();
            }
        });
    }

    /// Selects the next match down (`forward`) or up, wrapping.
    fn step(&mut self, forward: bool, view: Option<&Entity<TerminalView>>, cx: &mut App) {
        let Some(view) = view.filter(|_| self.open && self.error.is_none()) else {
            return;
        };
        view.update(cx, |view, cx| {
            if let Err(error) = view.find_next(forward, cx) {
                self.error = Some(error);
                view.set_find(None, cx).ok();
            }
        });
    }

    /// Follows the panel's active terminal, `view`, while the row is open:
    /// the terminal it left has its query cleared, and the new one gets it
    /// without its selection moving.
    pub(crate) fn follow(&mut self, view: Option<&Entity<TerminalView>>, cx: &mut App) {
        let old = self.view.as_ref().map(|old| old.entity_id());
        if !self.open || old == view.map(Entity::entity_id) {
            return;
        }
        self.set_view(view, cx);
        let Some(view) = view else {
            return;
        };
        if self.error.is_some() {
            // The query doesn't compile in any terminal.
            return;
        }
        let query = self.query(cx);
        view.update(cx, |view, cx| {
            self.unchecked = view.terminal().is_none() && !query.text.is_empty();
            self.error = view.set_find(Some(query), cx).err();
        });
    }

    /// Searches again in `view` once the session a query was set on
    /// unchecked has started: a query that doesn't compile then shows its
    /// error and is cleared there, and one that does selects its newest
    /// match.
    pub(crate) fn settle(&mut self, view: Option<&Entity<TerminalView>>, cx: &mut App) {
        let started = view.is_some_and(|view| view.read(cx).terminal().is_some());
        if self.unchecked && started {
            self.search(view, cx);
        }
    }

    /// Makes `view` the one the row's query is on, clearing the query of
    /// the one it was on before, if another.
    fn set_view(&mut self, view: Option<&Entity<TerminalView>>, cx: &mut App) {
        let old = self.view.take().and_then(|old| old.upgrade());
        if let Some(old) = old
            && Some(old.entity_id()) != view.map(Entity::entity_id)
        {
            old.update(cx, |old, cx| old.set_find(None, cx).ok());
        }
        self.view = view.map(Entity::downgrade);
    }

    /// What the row shows beside the input, and whether it's an error: the
    /// query's error, or "No results" while none of `view`'s visible rows
    /// match it.
    pub(crate) fn label(&self, view: Option<&TerminalView>) -> Option<(SharedString, bool)> {
        if let Some(error) = &self.error {
            return Some((error.to_string().into(), true));
        }
        let query = view?.find_query()?;
        match view?.terminal()?.find_all_visible(query) {
            Ok(matches) => matches.is_empty().then(|| ("No results".into(), false)),
            Err(error) => Some((error.to_string().into(), true)),
        }
    }

    /// The row, while it's open.
    pub(crate) fn render(
        &self,
        view: Option<&Entity<TerminalView>>,
        ui: &UiTheme,
        cx: &mut Context<TerminalPanel>,
    ) -> Option<impl IntoElement + use<>> {
        if !self.open {
            return None;
        }
        let label = self.label(view.map(|view| view.read(cx)));
        let toggles = [
            (FindOp::ToggleCase, "Aa", self.match_case),
            (FindOp::ToggleRegex, ".*", self.regex),
        ]
        .into_iter()
        .enumerate()
        .map(|(ix, (op, text, on))| {
            ToggleButton::new(("terminal-find-option", ix), text, ui)
                .on(on)
                .on_click(cx.listener(move |panel, _, window, cx| panel.run_find(op, window, cx)))
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
        let row = find_commands::handle_in(div(), cx, TerminalPanel::run_find)
            .key_context(find_bar::KEY_CONTEXT)
            .on_action(cx.listener(on_run_command))
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
            .child(div().flex_1().min_w_0().child(self.input.clone()))
            .children(label)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap_0p5()
                    // A toggle keeps focus where it was.
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .children(toggles),
            );
        Some(row)
    }
}

/// A typed command, as its action, the find commands among them; the
/// input's editing commands; others go on.
fn on_run_command(
    panel: &mut TerminalPanel,
    run: &RunCommand,
    window: &mut Window,
    cx: &mut Context<TerminalPanel>,
) {
    if kiln::commands::dispatch_typed(run, window, cx) {
        return;
    }
    let edit: fn(&mut TextInput, &mut Context<TextInput>) = match picker::op(run, cx) {
        Some(PaletteOp::Backspace) => TextInput::backspace,
        Some(PaletteOp::DeleteWordLeft) => TextInput::delete_word_left,
        Some(PaletteOp::Clear) => TextInput::clear,
        Some(PaletteOp::Paste) => TextInput::paste,
        _ => return cx.propagate(),
    };
    panel.update_find(|find, _, cx| find.input.update(cx, edit), cx);
}

#[cfg(test)]
#[path = "terminal_find_keys_tests.rs"]
mod terminal_find_keys_tests;
