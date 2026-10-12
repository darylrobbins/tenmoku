//! The editor's commands. Every key binding dispatches one GPUI action,
//! kiln's [`RunCommand`], carrying an [`Invocation`] (see
//! [`crate::keymap`]). The focused [`EditorView`] runs it through the
//! editor's registry and lets any other ID propagate to the app's registry,
//! which kiln owns; [`init`] adds vim's commands to it.

use std::{cell::Cell, ops::Range, ops::RangeInclusive, rc::Rc};

use fe_command::{
    Actor, CommandError, CommandId, CommandMeta, ContextPredicate, DispatchError, Dispatched,
    Effect, Invocation, Registry, Source, Value,
};
use fe_editor::commands::{Editor, EditorHost, EditorTarget, ScrollRequest, Viewport};
use fe_keymap::{CatalogError, CommandCatalog};
use fe_syntax::Syntax;
use gpui::{App, ClipboardItem, Context, Window};

use crate::{EditorView, completion_menu, view::synced_syntax, vim};

// The app's command plumbing is kiln's (docs/decisions/app-framework.md);
// fastedit keeps its paths to it.
pub(crate) use kiln_platform::commands::notify_observers;
pub use kiln_platform::commands::{AppCommands, RunCommand, observe_invocations};

/// The name app commands are titled with (`Hide fastedit`).
const APP_NAME: &str = "fastedit";

thread_local! {
    /// Built once: GPUI runs on one thread, and handlers aren't `Sync`.
    static EDITOR_COMMANDS: Rc<Registry<Editor>> = Rc::new(editor_registry());
}

/// Shows the hover popover at the caret ([`crate::hover`]).
pub const SHOW_HOVER: &str = "editor.show_hover";

/// Saves the buffer: fe-editor's command, which the view runs off the main
/// thread ([`EditorView::save_from_command`]).
const SAVE: &str = "editor.save";

/// fe-editor's commands, and the view's own: in the editor's context, and
/// the completion menu's in its own ([`completion_menu`]). The
/// view's do nothing to the model: once one has dispatched, the view does
/// its work ([`EditorView::run_command`]).
fn editor_registry() -> Registry<Editor> {
    let mut registry = fe_editor::commands::registry();
    registry
        .register(
            SHOW_HOVER,
            "Show Hover",
            |_: &mut EditorTarget<'_>, _: &Actor| Ok(Dispatched::new(Effect::Nothing)),
        )
        .expect("view command IDs are valid and distinct")
        .context(ContextPredicate::identifier(fe_editor::commands::CONTEXT));
    let menu = [
        (
            completion_menu::TRIGGER,
            "Trigger Completion",
            fe_editor::commands::CONTEXT,
        ),
        (
            completion_menu::SELECT_PREV,
            "Select Previous Completion",
            completion_menu::KEY_CONTEXT,
        ),
        (
            completion_menu::SELECT_NEXT,
            "Select Next Completion",
            completion_menu::KEY_CONTEXT,
        ),
        (
            completion_menu::ACCEPT,
            "Accept Completion",
            completion_menu::KEY_CONTEXT,
        ),
        (
            completion_menu::DISMISS,
            "Dismiss Completion",
            completion_menu::KEY_CONTEXT,
        ),
    ];
    for (id, title, context) in menu {
        registry
            .register(id, title, |_: &mut EditorTarget<'_>, _: &Actor| {
                Ok(Dispatched::new(Effect::Nothing))
            })
            .expect("view command IDs are valid and distinct")
            .context(ContextPredicate::identifier(context));
    }
    registry
}

type AppCommand = fn(&mut App);

/// vim's app commands: `vim.toggle`, and `vim.key`, which does nothing
/// when dispatched: a binding to it hands its keystroke to vim instead
/// ([`crate::vim`]).
fn register_vim_commands(app: &mut Registry<AppCommands>) {
    let commands: [(&str, &str, AppCommand); 2] = [
        (vim::TOGGLE, "Toggle Vim Mode", |cx| {
            let enabled = vim::vim_enabled(cx);
            vim::set_vim_enabled(cx, !enabled)
        }),
        (vim::KEY, "Vim Key", |_| {}),
    ];
    for (id, title, run) in commands {
        app.register(id, title, move |cx: &mut &mut App, _: &Actor| {
            run(cx);
            Ok(Dispatched::new(Effect::Nothing))
        })
        .expect("vim's app command IDs are valid and distinct");
    }
}

/// kiln's app commands and vim's, as [`init`] installs them.
fn app_registry() -> Registry<AppCommands> {
    let mut app = kiln_platform::commands::app_commands(APP_NAME);
    register_vim_commands(&mut app);
    app
}

/// The editor's, vim's and the app's commands, to check keymap bindings
/// against: pass it to [`fe_keymap::KeymapStore::load`].
pub fn command_catalog() -> Box<dyn CommandCatalog> {
    Box::new(Catalog {
        editor: EDITOR_COMMANDS.with(Rc::clone),
        vim: vim::registry(),
        app: app_registry(),
    })
}

struct Catalog {
    editor: Rc<Registry<Editor>>,
    vim: Rc<Registry<fe_vim::VimCommands>>,
    app: Registry<AppCommands>,
}

/// A command the editor doesn't know is checked against vim's, then the
/// app's.
impl CommandCatalog for Catalog {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        match self.editor.check(command, args) {
            Err(CatalogError::UnknownCommand) => match self.vim.check(command, args) {
                Err(CatalogError::UnknownCommand) => self.app.check(command, args),
                result => result,
            },
            result => result,
        }
    }
}

/// The editor's and vim's commands, for the command palette to list: it
/// lists each where its context holds.
pub fn command_list() -> Vec<CommandMeta> {
    let editor = EDITOR_COMMANDS.with(Rc::clone);
    let vim = vim::registry();
    (editor.commands().cloned())
        .chain(vim.commands().cloned())
        .collect()
}

/// Registers the app commands, vim's among them, and the handler that runs
/// them.
pub(crate) fn init(cx: &mut App) {
    kiln_platform::commands::init(cx, APP_NAME);
    kiln_platform::commands::register_app_commands(cx, register_vim_commands);
}

thread_local! {
    /// What the last frame of the view about to dispatch showed: set by
    /// [`EditorView::page_rows`], which every dispatch asks first.
    pub(crate) static VIEWPORT: Cell<Option<Viewport>> = const { Cell::new(None) };
    /// The first row a command asked to show, which the view takes in
    /// [`EditorView::dispatched`].
    static SCROLLED: Cell<Option<u32>> = const { Cell::new(None) };
    /// Whether a command asked for the viewport, as every scroll does,
    /// even one that leaves the top row alone.
    static VIEWPORT_ASKED: Cell<bool> = const { Cell::new(false) };
}

/// The editor's side of a dispatch: the view's clipboard, viewport and
/// syntax folds.
pub(crate) struct ViewHost<'a> {
    pub(crate) cx: &'a mut App,
    pub(crate) page_rows: u32,
    /// The syntax tree, if it matches the text.
    pub(crate) syntax: Option<&'a Syntax>,
}

impl EditorHost for ViewHost<'_> {
    fn page_rows(&self) -> u32 {
        self.page_rows
    }

    fn read_clipboard(&mut self) -> Option<String> {
        self.cx.read_from_clipboard().and_then(|item| item.text())
    }

    fn write_clipboard(&mut self, text: String) {
        self.cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn fold_range_at(&self, row: u32) -> Option<RangeInclusive<u32>> {
        self.syntax
            .and_then(|syntax| fe_syntax::fold_range_at(syntax, row))
    }

    fn fold_ranges(&self, rows: Range<u32>) -> Vec<RangeInclusive<u32>> {
        self.syntax
            .map(|syntax| fe_syntax::fold_ranges(syntax, rows))
            .unwrap_or_default()
    }

    fn viewport(&self) -> Option<Viewport> {
        VIEWPORT_ASKED.set(true);
        VIEWPORT.get()
    }

    fn scroll(&mut self, request: ScrollRequest) {
        match request {
            ScrollRequest::TopRow(row) => SCROLLED.set(Some(row)),
        }
    }
}

impl EditorView {
    pub(crate) fn on_run_command(
        &mut self,
        run: &RunCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_command(run.0.clone(), Some(window), cx);
    }

    /// Runs `invocation` on this editor, in its context: the stack it was
    /// dispatched in, such as `[workbench, editor]` from a key or a menu,
    /// if the editor's frame is in it, or else the editor's frame alone. An
    /// ID the editor doesn't know propagates, still in that context, to the
    /// app's commands. With vim on, vim's IDs run through its registry, the
    /// editor's frame stamped from its vim state, and in insert or replace
    /// mode an editor command other than text input splits the session
    /// ([`fe_vim::VimState::split_session`]). In replace mode typed text and
    /// `editor.backspace` overwrite and put back
    /// ([`fe_vim::VimState::replace_text`]), and `vim.normal_mode` is
    /// replace mode's `Esc`, `vim.replace_normal_mode`. While a macro
    /// records, what runs is recorded as the keys it was
    /// ([`EditorView::record_typing`]).
    ///
    /// Given the window, the view takes [`SAVE`] over from fe-editor's
    /// synchronous command: it saves off the main thread
    /// ([`EditorView::save_from_command`]).
    pub(crate) fn run_command(
        &mut self,
        invocation: Invocation,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) {
        let in_editor = ContextPredicate::identifier(fe_editor::commands::CONTEXT);
        let mut invocation = if in_editor.eval(&invocation.context) {
            invocation
        } else {
            invocation.context(fe_editor::commands::context())
        };
        self.sync_vim(cx);
        if self.vim.is_some() && vim::registry().get(&invocation.command).is_some() {
            self.record_vim_command(&invocation);
            if invocation.command.as_str() == vim::NORMAL_MODE && self.typing_mode() == Some(true) {
                invocation.command = vim::REPLACE_NORMAL_MODE
                    .parse()
                    .expect("a valid command ID");
            }
            invocation.context = vim::stamp(invocation.context, self.vim.as_ref());
            self.run_vim(&invocation, cx);
            return;
        }
        let registry = EDITOR_COMMANDS.with(Rc::clone);
        let replace = self.typing_mode();
        if replace.is_some() && registry.validate(&invocation).is_ok() {
            self.record_typing(&invocation);
        }
        let page_rows = self.page_rows();
        let syntax = synced_syntax(&self.syntax, &self.model);
        let mut host = ViewHost {
            cx,
            page_rows,
            syntax,
        };
        let meta = registry.get(&invocation.command);
        let mutates = meta.is_some_and(|meta| meta.mutates_buffer);
        let saves = window.is_some() && invocation.command.as_str() == SAVE;
        let mut dispatch = |model: &mut fe_editor::EditorModel| {
            if saves {
                // The view saves once it has dispatched.
                registry
                    .validate(&invocation)
                    .map(|_| Dispatched::new(Effect::Nothing))
            } else {
                registry.dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
            }
        };
        let mut split = false;
        let result = match &mut self.vim {
            // In insert mode, `Enter` is a typed line break, and any other
            // editor command but text input splits the session (§9.8). An
            // invocation the registry would refuse does neither. So in
            // replace mode, where typing and `<BS>` are fe-vim's.
            Some(state) if replace.is_some() && meta.is_some() => {
                match registry.validate(&invocation) {
                    Err(error) => Err(error),
                    Ok(_) => match invocation.command.as_str() {
                        vim::NEWLINE => vim::line_break(state, &mut self.model, &invocation),
                        vim::INSERT if replace == Some(true) => {
                            let text = invocation.args.get("text").and_then(Value::as_str);
                            let text = text.unwrap_or_default();
                            vim::replace_text(state, &mut self.model, &invocation, text)
                        }
                        vim::BACKSPACE if replace == Some(true) => {
                            vim::replace_backspace(state, &mut self.model, &invocation)
                        }
                        // Edit nothing, or as typing does, so the session
                        // goes on.
                        SHOW_HOVER => dispatch(&mut self.model),
                        command
                            if command == completion_menu::TRIGGER
                                || command.starts_with("completion.") =>
                        {
                            dispatch(&mut self.model)
                        }
                        command if vim::TEXT_INPUT.contains(&command) => dispatch(&mut self.model),
                        command => {
                            split = true;
                            let splits = if mutates {
                                fe_vim::Splits::Edits
                            } else if vim::MOTION_PREFIXES
                                .iter()
                                .any(|prefix| command.starts_with(prefix))
                            {
                                fe_vim::Splits::Moves
                            } else {
                                fe_vim::Splits::Other
                            };
                            state.split_session(&mut self.model, splits, dispatch)
                        }
                    },
                }
            }
            _ => dispatch(&mut self.model),
        };
        if split {
            // Ending the session may have dropped indentation, whatever the
            // command did.
            self.changed(cx);
        }
        match self.dispatched(result, &invocation, mutates, cx) {
            Ok(()) if invocation.command.as_str() == SHOW_HOVER => self.show_hover(cx),
            Ok(()) if saves => {
                if let Some(window) = window {
                    self.save_from_command(window, cx);
                }
            }
            Ok(()) => {
                if !self.run_completion_command(invocation.command.as_str(), cx) {
                    self.sync_completion(cx);
                }
            }
            Err(DispatchError::UnknownCommand(_)) => {
                kiln_platform::commands::propagate_with_context(invocation, cx);
            }
            Err(error) => log::error!("{error}"),
        }
    }

    /// Applies a dispatch's effect and tells the observers, or logs a
    /// failed command; `mutates` is whether the command can edit. Returns
    /// any other error.
    pub(crate) fn dispatched<Tx>(
        &mut self,
        result: Result<Dispatched<Tx>, DispatchError>,
        invocation: &Invocation,
        mutates: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), DispatchError> {
        let ok = result.is_ok();
        match result {
            Ok(dispatched) => {
                match dispatched.effect {
                    Effect::Changed => self.changed(cx),
                    Effect::Notify => cx.notify(),
                    Effect::Nothing => {}
                }
                notify_observers(invocation, cx);
                self.write_primary(cx);
            }
            Err(DispatchError::Failed {
                error: CommandError(message),
                ..
            }) => {
                log::error!("{message}");
                // A failed edit, such as undo, may have moved the carets;
                // a failed save only repaints.
                if mutates {
                    self.changed(cx);
                } else {
                    cx.notify();
                }
            }
            Err(error) => {
                SCROLLED.set(None);
                VIEWPORT_ASKED.set(false);
                return Err(error);
            }
        }
        let asked = VIEWPORT_ASKED.take();
        match SCROLLED.take() {
            Some(top_row) => self.scroll_to_top_row(top_row),
            // A scroll that kept the top row still moved the caret, which
            // mustn't scroll the view back; so a motion that asked (`H`)
            // and left it in view. One that took it out of view (`G`, `'a`)
            // scrolls it in.
            None if ok && asked && self.shows_caret() => self.pin_top_row(),
            None => {}
        }
        Ok(())
    }

    /// Whether the primary caret's display row is among the rows the view
    /// shows, as many as fit whole: from the display row a vim scroll put
    /// first since the last frame, or else that frame's first row, partway
    /// down a wrapped row as it may be. A command that kept the view
    /// ([`EditorView::pin_top_row`]) leaves either.
    fn shows_caret(&self) -> bool {
        let Some(layout) = &self.layout else {
            return false;
        };
        let frame = &layout.frame;
        let top = self.scrolled_display_top.unwrap_or(frame.first_row);
        let display = self.model.display();
        let caret = display.row_for_offset(self.model.primary_selection().head());
        (top..top.saturating_add(frame.visible_rows())).contains(&caret)
    }

    /// Types `text` at every caret, as `editor.insert` from the keyboard.
    pub(crate) fn type_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let invocation = Invocation::of("editor.insert")
            .expect("valid command ID")
            .args(Value::from_iter([("text", text)]))
            .source(Source::Keyboard);
        self.run_command(invocation, None, cx);
        // Replace mode overwrites: nothing to complete.
        if self.typing_mode() != Some(true) {
            self.completion_typed(text, cx);
        }
    }
}
