//! GPUI's side of `fe-command`. Every key binding dispatches one GPUI
//! action, [`RunCommand`], carrying an [`Invocation`] (see
//! [`crate::keymap`]). The focused
//! [`EditorView`] runs it through the editor's registry and lets any other
//! ID propagate to the app's registry, which [`init`] owns.

use std::{ops::Range, ops::RangeInclusive, rc::Rc};

use fe_command::{
    Actor, CommandError, CommandId, CommandTarget, ContextPredicate, DispatchError, Dispatched,
    Effect, Invocation, Registry, Source, Value,
};
use fe_editor::commands::{Editor, EditorHost, EditorTarget};
use fe_keymap::{CatalogError, CommandCatalog};
use fe_syntax::Syntax;
use gpui::{Action, App, ClipboardItem, Context, Global, Window};

use crate::{EditorView, view::synced_syntax, vim};

/// Runs a command: the one GPUI action every binding dispatches.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = fastedit, no_json)]
pub struct RunCommand(pub Invocation);

/// The [`CommandTarget`] of app commands, such as `app.quit`: the whole
/// GPUI app.
pub struct AppCommands;

impl CommandTarget for AppCommands {
    type Tx = ();
    type Target<'a> = &'a mut App;
}

type Observer = Rc<dyn Fn(&Invocation)>;

#[derive(Default)]
struct Commands {
    app: Rc<Registry<AppCommands>>,
    observers: Vec<Observer>,
    /// The invocation a focused view propagated, stamped with its
    /// context: GPUI propagates the original [`RunCommand`], whose context
    /// is empty. Cleared once the dispatch is over.
    propagated: Option<Invocation>,
}

impl Global for Commands {}

thread_local! {
    /// Built once: GPUI runs on one thread, and handlers aren't `Sync`.
    static EDITOR_COMMANDS: Rc<Registry<Editor>> = Rc::new(fe_editor::commands::registry());
}

type AppCommand = fn(&mut App);

/// The app commands, in no context. The window ones act on the active
/// window, and do nothing without one. `vim.key` does nothing when
/// dispatched: a binding to it hands its keystroke to vim instead
/// ([`crate::vim`]).
fn app_registry() -> Registry<AppCommands> {
    let mut app = Registry::new();
    let commands: [(&str, &str, AppCommand); 10] = [
        ("app.quit", "Quit", |cx| cx.quit()),
        ("app.hide", "Hide fastedit", |cx| cx.hide()),
        ("app.hide_others", "Hide Others", |cx| cx.hide_other_apps()),
        ("app.show_all", "Show All", |cx| cx.unhide_other_apps()),
        ("window.minimize", "Minimize", |cx| {
            on_active_window(cx, |window| window.minimize_window())
        }),
        ("window.zoom", "Zoom", |cx| {
            on_active_window(cx, |window| window.zoom_window())
        }),
        ("window.toggle_fullscreen", "Toggle Full Screen", |cx| {
            on_active_window(cx, |window| window.toggle_fullscreen())
        }),
        ("window.close", "Close Window", |cx| {
            on_active_window(cx, |window| window.remove_window())
        }),
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
        .expect("app command IDs are valid and distinct");
    }
    app
}

/// Runs `f` on the active window, if any, once the current dispatch is
/// over: a key or menu item runs its command while GPUI has that window
/// out for the dispatch, so it can't be updated until then.
fn on_active_window(cx: &mut App, f: fn(&mut Window)) {
    if let Some(handle) = cx.active_window() {
        cx.defer(move |cx| {
            handle.update(cx, |_, window, _| f(window)).ok();
        });
    }
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

/// Registers the app commands and the handler that runs them.
pub(crate) fn init(cx: &mut App) {
    cx.default_global::<Commands>().app = Rc::new(app_registry());
    cx.on_action(|run: &RunCommand, cx| {
        let propagated = cx.default_global::<Commands>().propagated.take();
        // The view's stamped copy, if this is what it propagated.
        let invocation = propagated
            .filter(|stamped| {
                Invocation {
                    context: run.0.context.clone(),
                    ..stamped.clone()
                } == run.0
            })
            .unwrap_or_else(|| run.0.clone());
        dispatch_app(&invocation, cx)
    });
}

/// Calls `observer` with every invocation the editor and app registries
/// dispatch successfully, after it ran: the seam for a macro recorder, a
/// logger or an agent bridge.
pub fn observe_invocations(cx: &mut App, observer: impl Fn(&Invocation) + 'static) {
    cx.default_global::<Commands>()
        .observers
        .push(Rc::new(observer));
}

pub(crate) fn notify_observers(invocation: &Invocation, cx: &App) {
    if let Some(commands) = cx.try_global::<Commands>() {
        for observer in &commands.observers {
            observer(invocation);
        }
    }
}

/// Runs an app command. Reached by whatever no focused view handled, and
/// by a menu item chosen with no window open.
pub(crate) fn dispatch_app(invocation: &Invocation, cx: &mut App) {
    let Some(registry) = cx
        .try_global::<Commands>()
        .map(|commands| commands.app.clone())
    else {
        return;
    };
    let mut target = &mut *cx;
    match registry.dispatch(&mut target, invocation) {
        Ok(_) => notify_observers(invocation, cx),
        Err(error) => log::warn!("{error}"),
    }
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
}

impl EditorView {
    pub(crate) fn on_run_command(
        &mut self,
        run: &RunCommand,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_command(run.0.clone(), cx);
    }

    /// Runs `invocation` on this editor, in its context: the stack it was
    /// dispatched in, such as `[workbench, editor]` from a key or a menu,
    /// if the editor's frame is in it, or else the editor's frame alone. An
    /// ID the editor doesn't know propagates, still in that context, to the
    /// app's commands. With vim on, vim's IDs run through its registry, the
    /// editor's frame stamped from its vim state, and in insert mode an
    /// editor command other than text input splits the session
    /// ([`fe_vim::VimState::split_session`]).
    fn run_command(&mut self, invocation: Invocation, cx: &mut Context<Self>) {
        let in_editor = ContextPredicate::identifier(fe_editor::commands::CONTEXT);
        let mut invocation = if in_editor.eval(&invocation.context) {
            invocation
        } else {
            invocation.context(fe_editor::commands::context())
        };
        self.sync_vim(cx);
        if self.vim.is_some() && vim::registry().get(&invocation.command).is_some() {
            invocation.context = vim::stamp(invocation.context, self.vim.as_ref());
            self.run_vim(&invocation, cx);
            return;
        }
        let registry = EDITOR_COMMANDS.with(Rc::clone);
        let page_rows = self.page_rows();
        let syntax = synced_syntax(&self.syntax, &self.model);
        let mut host = ViewHost {
            cx,
            page_rows,
            syntax,
        };
        let meta = registry.get(&invocation.command);
        let mutates = meta.is_some_and(|meta| meta.mutates_buffer);
        let mut dispatch = |model: &mut fe_editor::EditorModel| {
            registry.dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
        };
        let mut split = false;
        let result = match &mut self.vim {
            // In insert mode, `Enter` is a typed line break, and any other
            // editor command but text input splits the session (§9.8).
            Some(state) if state.mode() == fe_vim::Mode::Insert && meta.is_some() => {
                match invocation.command.as_str() {
                    vim::NEWLINE => vim::line_break(state, &mut self.model, &invocation),
                    command if vim::TEXT_INPUT.contains(&command) => dispatch(&mut self.model),
                    _ => {
                        split = true;
                        state.split_session(&mut self.model, mutates, dispatch)
                    }
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
            Ok(()) => {}
            Err(DispatchError::UnknownCommand(_)) => {
                cx.default_global::<Commands>().propagated = Some(invocation);
                // In case no app handler takes it.
                cx.defer(|cx| cx.default_global::<Commands>().propagated = None);
                cx.propagate();
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
        match result {
            Ok(dispatched) => {
                match dispatched.effect {
                    Effect::Changed => self.changed(cx),
                    Effect::Notify => cx.notify(),
                    Effect::Nothing => {}
                }
                notify_observers(invocation, cx);
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
            Err(error) => return Err(error),
        }
        Ok(())
    }

    /// Types `text` at every caret, as `editor.insert` from the keyboard.
    pub(crate) fn type_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let invocation = Invocation::of("editor.insert")
            .expect("valid command ID")
            .args(Value::from_iter([("text", text)]))
            .source(Source::Keyboard);
        self.run_command(invocation, cx);
    }
}
