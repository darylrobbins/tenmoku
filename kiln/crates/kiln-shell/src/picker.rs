// SPDX-License-Identifier: LicenseRef-Proprietary

//! A palette that picks one of a list of matches: a [`TextInput`] whose
//! text a [`PickerDelegate`] matches, the matches below it, one selected,
//! in kiln-ui's [`PaletteFrame`]. A go-to-file palette is a delegate, and
//! so is a command palette.
//!
//! A click on a row picks it at once, which is why the rows aren't kiln-ui's
//! [`kiln_ui::List`] (a click there selects, a double click confirms).
//!
//! The keys are the commands of [`registry`], which [`crate::register`]
//! installs: the picker's (`picker.select_prev`, `picker.select_next`,
//! `picker.confirm`, `picker.dismiss`) in the [`KEY_CONTEXT`] context, and
//! its input's (`text_input.backspace`, `text_input.delete_word_left`,
//! `text_input.clear`, `text_input.paste`) in [`TextInput::KEY_CONTEXT`].
//! Other views with a [`TextInput`] can run the latter through [`op`] and
//! [`PickerOp::edit`]. A key reaches them as a [`RunCommand`]; a command
//! the picker doesn't know goes on to its host. Opening a palette is the
//! host's command.
//!
//! ## Vim keys
//!
//! With the host's switch on ([`set_vim_keys`]), the picker has telescope's
//! two modes. [`Picker::show`] opens it in insert mode, which is as with the
//! switch off but for `picker.dismiss` (escape) and an unbound `ctrl-[`:
//! they go to normal mode rather than dismissing. In normal mode the query
//! is kept but takes no text ([`TextInput::set_typing`]; focus stays on the
//! input): `j` and `k` move the selection, `g g` selects the first match and
//! `G` the last (a `g` then anything else is dropped), `picker.confirm`
//! (enter) accepts, `picker.dismiss`, `ctrl-[` and `q` dismiss, and `i`,
//! `a`, `I` and `A` go back to insert mode, typing appending to the query.
//! The other picker commands work as in insert mode; the input's do
//! nothing. The letters reach the picker's key listener only when no
//! binding matched them. The switch is read live: once it's off, the next
//! keystroke or frame is back in insert mode.
//!
//! While the switch is on, the picker's key context also sets `vim_mode` to
//! `insert` or `normal`, so a host keymap can bind more keys in either.

use std::{ops::Range, rc::Rc};

use gpui::Bounds;
use gpui::{
    AnyElement, App, AppContext as _, Context, DismissEvent, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, Global, InteractiveElement as _, IntoElement, KeyContext, KeyDownEvent,
    ParentElement as _, Render, ScrollStrategy, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, UniformListScrollHandle, Window, div, prelude::FluentBuilder as _,
    uniform_list,
};
use kiln_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use kiln_core::CommandCatalog;
use kiln_platform::commands::RunCommand;
use kiln_ui::{PaletteFrame, ROW_HEIGHT, TextInput, TextInputEvent, UiTheme, ui_theme};

/// The key context a picker's element sets.
pub const KEY_CONTEXT: &str = "picker";

/// The key context key that holds the vim mode, `insert` or `normal`,
/// while the vim switch is on.
pub const VIM_MODE: &str = "vim_mode";

/// Rows shown before the list scrolls.
pub const VISIBLE_ROWS: usize = 12;

/// What a [`Picker`] picks from.
pub trait PickerDelegate: Sized + 'static {
    /// Matches `query`, the input's text. Once the matches change, maybe
    /// later from a task, call [`Picker::matches_updated`].
    fn update_matches(&mut self, query: String, cx: &mut Context<Picker<Self>>);

    fn match_count(&self) -> usize;

    /// Match `ix`'s identity, distinct among the matches. Its row's element
    /// and click follow it rather than the row's position, so a click
    /// pressed on one match never picks another the results moved there.
    fn match_id(&self, ix: usize) -> SharedString;

    fn render_match(&self, ix: usize, selected: bool, cx: &App) -> AnyElement;

    /// Match `ix` was picked: whether it's taken, the picker then being
    /// dismissed. A refused pick leaves the picker open.
    fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Picker<Self>>) -> bool;

    /// The picker was dismissed without a pick.
    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>);

    /// The status line under the input.
    fn status(&self) -> Option<SharedString> {
        None
    }
}

/// A picker's vim mode: see the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Typing filters, as with vim off.
    #[default]
    Insert,
    /// Keys move and pick; nothing is typed.
    Normal,
}

impl Mode {
    /// Its name in the key context's [`VIM_MODE`].
    pub fn name(self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Normal => "normal",
        }
    }
}

/// The host's vim switch, read live.
struct VimKeys(fn(&App) -> bool);

impl Global for VimKeys {}

/// Installs the host's vim switch, which pickers read on each keystroke
/// and frame. Without one, vim keys are off.
pub fn set_vim_keys(cx: &mut App, enabled: fn(&App) -> bool) {
    cx.set_global(VimKeys(enabled));
}

/// Whether pickers take vim keys: the switch [`set_vim_keys`] installed.
pub fn vim_keys(cx: &App) -> bool {
    cx.try_global::<VimKeys>()
        .is_some_and(|switch| (switch.0)(cx))
}

/// A picker over `D`'s matches. It emits [`DismissEvent`] once confirmed
/// or dismissed, for its host to hide it.
pub struct Picker<D: PickerDelegate> {
    pub delegate: D,
    input: Entity<TextInput>,
    selected: usize,
    mode: Mode,
    /// Normal mode's `g` waits for a second.
    pending_g: bool,
    scroll: UniformListScrollHandle,
    _input: Subscription,
}

impl<D: PickerDelegate> EventEmitter<DismissEvent> for Picker<D> {}

impl<D: PickerDelegate> Picker<D> {
    /// A picker over `delegate`'s matches. It draws in the active theme's
    /// colours ([`ui_theme`]), so it follows a theme change; `_ui` is
    /// unused, left for callers until a later API cleanup.
    pub fn new(delegate: D, placeholder: &str, _ui: UiTheme, cx: &mut Context<Self>) -> Self {
        let placeholder = SharedString::from(placeholder.to_owned());
        let input = cx.new(|cx| TextInput::new(placeholder, &ui_theme(cx), cx));
        let changed = cx.subscribe(&input, |picker, input, event, cx| {
            if *event == TextInputEvent::Changed {
                let query = input.read(cx).text().to_owned();
                picker.delegate.update_matches(query, cx);
            }
        });
        Self {
            delegate,
            input,
            selected: 0,
            mode: Mode::Insert,
            pending_g: false,
            scroll: UniformListScrollHandle::new(),
            _input: changed,
        }
    }

    pub fn input(&self) -> &Entity<TextInput> {
        &self.input
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Its vim mode, insert unless the switch is on: see the module docs.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Clears the input, focuses it and matches the empty query, in insert
    /// mode.
    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_text("", cx));
        self.set_mode(Mode::Insert, cx);
        self.selected = 0;
        window.focus(&self.input.focus_handle(cx), cx);
        self.delegate.update_matches(String::new(), cx);
        cx.notify();
    }

    /// For the delegate once its matches change: those of a new query
    /// select the first, others keep the selection where it's still in
    /// range.
    pub fn matches_updated(&mut self, new_query: bool, cx: &mut Context<Self>) {
        let ix = if new_query { 0 } else { self.selected };
        self.select(ix, cx);
    }

    /// Selects row `ix`, clamped to the rows, and scrolls it into view.
    pub fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.selected = ix.min(self.delegate.match_count().saturating_sub(1));
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    pub fn confirm(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix < self.delegate.match_count() && self.delegate.confirm(ix, window, cx) {
            cx.emit(DismissEvent);
        }
    }

    /// Confirms the match a clicked row showed, wherever it is now, or
    /// nothing if it's gone.
    fn confirm_id(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.delegate.match_count();
        if let Some(ix) = (0..count).find(|&ix| self.delegate.match_id(ix) == id) {
            self.confirm(ix, window, cx);
        }
    }

    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.delegate.dismissed(window, cx);
        cx.emit(DismissEvent);
    }

    fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.pending_g = false;
        if self.mode != mode {
            self.mode = mode;
            cx.notify();
        }
        let typing = mode == Mode::Insert;
        self.input
            .update(cx, |input, cx| input.set_typing(typing, cx));
    }

    /// Back to insert mode if the vim switch has been turned off.
    fn sync_mode(&mut self, cx: &mut Context<Self>) {
        if self.mode == Mode::Normal && !vim_keys(cx) {
            self.set_mode(Mode::Insert, cx);
        }
    }

    /// Escape with vim keys on: to normal mode from insert, else dismissed.
    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Insert => self.set_mode(Mode::Normal, cx),
            Mode::Normal => self.dismiss(window, cx),
        }
    }

    /// Runs a picker or input command; others go on to the host.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        let Some(op) = op(run, cx) else {
            return cx.propagate();
        };
        self.sync_mode(cx);
        self.pending_g = false;
        match op {
            PickerOp::SelectPrev => self.select(self.selected.saturating_sub(1), cx),
            PickerOp::SelectNext => self.select(self.selected + 1, cx),
            PickerOp::Confirm => self.confirm(self.selected, window, cx),
            PickerOp::Dismiss if vim_keys(cx) => self.escape(window, cx),
            PickerOp::Dismiss => self.dismiss(window, cx),
            edit => {
                if let Some(edit) = edit.edit()
                    && self.mode == Mode::Insert
                {
                    self.input.update(cx, edit);
                }
            }
        }
    }

    /// A keystroke no binding matched: vim's keys, with the switch on.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if self.mode == Mode::Normal && !vim_keys(cx) {
            // Switched off in normal mode: this key types, though the frame
            // it arrived in registered no input handler.
            self.set_mode(Mode::Insert, cx);
            if let Some(text) = &keystroke.key_char {
                self.input.update(cx, |input, cx| input.insert(text, cx));
                cx.stop_propagation();
            }
            return;
        }
        if !vim_keys(cx) {
            return;
        }
        let m = keystroke.modifiers;
        let escape = (keystroke.key == "escape" && !m.modified())
            || (keystroke.key == "[" && m.control && !(m.alt || m.platform || m.shift));
        if escape {
            self.escape(window, cx);
            return cx.stop_propagation();
        }
        if self.mode == Mode::Insert || m.control || m.alt || m.platform || m.function {
            return;
        }
        let Some(key) = keystroke.key_char.as_deref() else {
            return;
        };
        let pending_g = std::mem::take(&mut self.pending_g);
        match key {
            "j" => self.select(self.selected + 1, cx),
            "k" => self.select(self.selected.saturating_sub(1), cx),
            "g" if pending_g => self.select(0, cx),
            "g" => self.pending_g = true,
            "G" => self.select(usize::MAX, cx),
            "q" => self.dismiss(window, cx),
            "i" | "a" | "I" | "A" => self.set_mode(Mode::Insert, cx),
            // Anything else is dropped: normal mode types nothing.
            _ => {}
        }
        cx.stop_propagation();
    }

    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let ui = ui_theme(cx);
        range
            .map(|ix| {
                let selected = ix == self.selected;
                let id = self.delegate.match_id(ix);
                div()
                    .id(ElementId::Name(id.clone()))
                    .h(ROW_HEIGHT)
                    .px_3()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .when(selected, |row| row.bg(ui.row_selected))
                    .hover(|row| row.bg(ui.row_hover))
                    .on_click(
                        cx.listener(move |picker, _, window, cx| {
                            picker.confirm_id(&id, window, cx)
                        }),
                    )
                    .child(self.delegate.render_match(ix, selected, cx))
                    .into_any_element()
            })
            .collect()
    }

    /// Where the list was laid out in the last frame, for tests.
    pub fn list_bounds(&self) -> Bounds<gpui::Pixels> {
        self.scroll.0.borrow().base_handle.bounds()
    }

    /// Where row `ix` was laid out in the last frame, from the list's
    /// bounds, scroll offset and measured row height, for tests.
    pub fn row_bounds(&self, ix: usize) -> Option<Bounds<gpui::Pixels>> {
        let count = self.delegate.match_count();
        let state = self.scroll.0.borrow();
        let list = state.base_handle.bounds();
        let height = state.last_item_size?.contents.height / count as f32;
        let top = list.origin.y + state.base_handle.offset().y + height * ix as f32;
        Some(Bounds::new(
            gpui::point(list.origin.x, top),
            gpui::size(list.size.width, height),
        ))
    }
}

impl<D: PickerDelegate> Focusable for Picker<D> {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl<D: PickerDelegate> Render for Picker<D> {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_mode(cx);
        let mut context = KeyContext::default();
        context.add(KEY_CONTEXT);
        if vim_keys(cx) {
            context.set(VIM_MODE, self.mode.name());
        }
        let count = self.delegate.match_count();
        let list = (count > 0).then(|| {
            uniform_list(
                "picker-rows",
                count,
                cx.processor(|picker, range, _, cx| picker.render_rows(range, cx)),
            )
            .track_scroll(&self.scroll)
            .h(ROW_HEIGHT * count.min(VISIBLE_ROWS) as f32)
        });
        let frame = PaletteFrame::new(&ui_theme(cx))
            .input(self.input.clone())
            .when_some(self.delegate.status(), PaletteFrame::status)
            .when_some(list, PaletteFrame::list);
        div()
            .key_context(context)
            .on_action(cx.listener(Self::on_run_command))
            .on_key_down(cx.listener(Self::on_key_down))
            .absolute()
            .inset_0()
            .child(frame)
    }
}

/// What a picker or input command asks for. Its handler only names it:
/// the picker, or another view with a [`TextInput`], then does it with the
/// window at hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerOp {
    SelectPrev,
    SelectNext,
    Confirm,
    Dismiss,
    Backspace,
    DeleteWordLeft,
    Clear,
    Paste,
}

impl PickerOp {
    /// The input edit an input command makes; `None` for the picker's own.
    pub fn edit(self) -> Option<fn(&mut TextInput, &mut Context<TextInput>)> {
        match self {
            Self::Backspace => Some(TextInput::backspace),
            Self::DeleteWordLeft => Some(TextInput::delete_word_left),
            Self::Clear => Some(TextInput::clear),
            Self::Paste => Some(TextInput::paste),
            Self::SelectPrev | Self::SelectNext | Self::Confirm | Self::Dismiss => None,
        }
    }
}

/// The [`CommandTarget`] of picker commands: the op to do.
pub struct PickerCommands;

impl CommandTarget for PickerCommands {
    type Tx = ();
    type Target<'a> = Option<PickerOp>;
}

struct Commands(Rc<Registry<PickerCommands>>);

impl Global for Commands {}

/// Installs the picker's commands; [`crate::register`] calls it.
pub fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

/// The picker's and its input's commands: see the module docs.
pub fn registry() -> Registry<PickerCommands> {
    use PickerOp::*;
    let input = TextInput::KEY_CONTEXT;
    let commands = [
        (
            "picker.select_prev",
            "Select Previous",
            KEY_CONTEXT,
            SelectPrev,
        ),
        ("picker.select_next", "Select Next", KEY_CONTEXT, SelectNext),
        ("picker.confirm", "Confirm", KEY_CONTEXT, Confirm),
        ("picker.dismiss", "Dismiss", KEY_CONTEXT, Dismiss),
        ("text_input.backspace", "Delete Backward", input, Backspace),
        (
            "text_input.delete_word_left",
            "Delete Word Backward",
            input,
            DeleteWordLeft,
        ),
        ("text_input.clear", "Clear", input, Clear),
        ("text_input.paste", "Paste", input, Paste),
    ];
    let mut registry = Registry::new();
    for (id, title, context, op) in commands {
        registry
            .register(
                id,
                title,
                move |target: &mut Option<PickerOp>, _: &Actor| {
                    *target = Some(op);
                    Ok(Dispatched::new(Effect::Notify))
                },
            )
            .expect("picker command IDs are valid and distinct")
            .context(ContextPredicate::identifier(context));
    }
    registry
}

/// The picker's commands, to check keymap bindings against.
pub fn catalog() -> Box<dyn CommandCatalog> {
    Box::new(registry())
}

/// The op of `run`, if it's a picker or input command that applies in its
/// context, once [`init`] has run.
pub fn op(run: &RunCommand, cx: &App) -> Option<PickerOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}
