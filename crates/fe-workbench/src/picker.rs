//! A palette that picks one of a list of matches: a [`TextInput`] whose
//! text a [`PickerDelegate`] matches, the matches below it, one selected.
//! Quick open is a delegate; the command palette will be another.
//!
//! The keys are the commands of [`init`]'s registry: the picker's in the
//! [`KEY_CONTEXT`] context, its input's in [`TextInput::KEY_CONTEXT`], and
//! the toggles of the palettes built on it in the workbench's.

use std::{ops::Range, rc::Rc};

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use fe_ui::{PaletteFrame, ROW_HEIGHT, TextInput, TextInputEvent, UiTheme};
use gpui::{
    AnyElement, App, AppContext as _, Context, DismissEvent, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, Global, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, UniformListScrollHandle, Window, div, prelude::FluentBuilder as _, uniform_list,
};

/// The key context a picker's element sets.
pub(crate) const KEY_CONTEXT: &str = "picker";

/// Rows shown before the list scrolls.
const VISIBLE_ROWS: usize = 12;

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

/// Emits [`DismissEvent`] once confirmed or dismissed, for its host to
/// hide it.
pub struct Picker<D: PickerDelegate> {
    pub delegate: D,
    input: Entity<TextInput>,
    selected: usize,
    scroll: UniformListScrollHandle,
    ui: UiTheme,
    _input: Subscription,
}

impl<D: PickerDelegate> EventEmitter<DismissEvent> for Picker<D> {}

impl<D: PickerDelegate> Picker<D> {
    pub fn new(delegate: D, placeholder: &str, ui: UiTheme, cx: &mut Context<Self>) -> Self {
        let placeholder = SharedString::from(placeholder.to_owned());
        let input = cx.new(|cx| TextInput::new(placeholder, &ui, cx));
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
            scroll: UniformListScrollHandle::new(),
            ui,
            _input: changed,
        }
    }

    pub fn input(&self) -> &Entity<TextInput> {
        &self.input
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Clears the input, focuses it and matches the empty query.
    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_text("", cx));
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
    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
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

    /// Runs a picker or input command; others go on to the workbench.
    fn on_run_command(&mut self, run: &RunCommand, window: &mut Window, cx: &mut Context<Self>) {
        let edit: fn(&mut TextInput, &mut Context<TextInput>) = match op(run, cx) {
            Some(PaletteOp::Backspace) => TextInput::backspace,
            Some(PaletteOp::DeleteWordLeft) => TextInput::delete_word_left,
            Some(PaletteOp::Clear) => TextInput::clear,
            Some(PaletteOp::Paste) => TextInput::paste,
            Some(PaletteOp::SelectPrev) => return self.select(self.selected.saturating_sub(1), cx),
            Some(PaletteOp::SelectNext) => return self.select(self.selected + 1, cx),
            Some(PaletteOp::Confirm) => return self.confirm(self.selected, window, cx),
            Some(PaletteOp::Dismiss) => return self.dismiss(window, cx),
            Some(PaletteOp::ToggleQuickOpen) | None => return cx.propagate(),
        };
        self.input.update(cx, edit);
    }

    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
                    .when(selected, |row| row.bg(self.ui.row_selected))
                    .hover(|row| row.bg(self.ui.row_hover))
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

    /// Where row `ix` was laid out in the last frame, from the list's
    /// bounds, scroll offset and measured row height.
    #[cfg(test)]
    pub(crate) fn row_bounds(&self, ix: usize) -> Option<gpui::Bounds<gpui::Pixels>> {
        let count = self.delegate.match_count();
        let state = self.scroll.0.borrow();
        let list = state.base_handle.bounds();
        let height = state.last_item_size?.contents.height / count as f32;
        let top = list.origin.y + state.base_handle.offset().y + height * ix as f32;
        Some(gpui::Bounds::new(
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
        let frame = PaletteFrame::new(&self.ui)
            .input(self.input.clone())
            .when_some(self.delegate.status(), PaletteFrame::status)
            .when_some(list, PaletteFrame::list);
        div()
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(Self::on_run_command))
            .absolute()
            .inset_0()
            .child(frame)
    }
}

/// What a palette command asks for. Its handler only names it: the picker,
/// or for a toggle the workbench, then does it with the window at hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaletteOp {
    ToggleQuickOpen,
    SelectPrev,
    SelectNext,
    Confirm,
    Dismiss,
    Backspace,
    DeleteWordLeft,
    Clear,
    Paste,
}

/// The [`CommandTarget`] of palette commands: the op to do.
pub(crate) struct PaletteCommands;

impl CommandTarget for PaletteCommands {
    type Tx = ();
    type Target<'a> = Option<PaletteOp>;
}

struct Commands(Rc<Registry<PaletteCommands>>);

impl Global for Commands {}

/// Registers the palette commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<PaletteCommands> {
    use PaletteOp::*;
    let input = TextInput::KEY_CONTEXT;
    let commands = [
        (
            "quick_open.toggle",
            "Go to File",
            crate::KEY_CONTEXT,
            ToggleQuickOpen,
        ),
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
                move |target: &mut Option<PaletteOp>, _: &Actor| {
                    *target = Some(op);
                    Ok(Dispatched::new(Effect::Notify))
                },
            )
            .expect("palette command IDs are valid and distinct")
            .context(ContextPredicate::identifier(context));
    }
    registry
}

/// The op of `run`, if it's a palette command that applies in its context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<PaletteOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}
