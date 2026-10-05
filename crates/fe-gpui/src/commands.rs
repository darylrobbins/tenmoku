//! GPUI's side of `fe-command`. Every key binding dispatches one GPUI
//! action, [`RunCommand`], carrying an [`Invocation`]. The focused
//! [`EditorView`] runs it through the editor's registry and lets any other
//! ID propagate to the app's registry, which [`init`] owns.

use std::{ops::Range, ops::RangeInclusive, rc::Rc};

use fe_command::{
    Actor, CommandError, CommandTarget, DispatchError, Dispatched, Effect, Invocation, Registry,
    Source, Value,
};
use fe_editor::commands::{Editor, EditorHost, EditorTarget};
use fe_syntax::Syntax;
use gpui::{Action, App, ClipboardItem, Context, Global, KeyBinding, Window};

use crate::{EditorView, default_bindings::DEFAULT_BINDINGS, view::synced_syntax};

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
}

impl Global for Commands {}

thread_local! {
    /// Built once: GPUI runs on one thread, and handlers aren't `Sync`.
    static EDITOR_COMMANDS: Rc<Registry<Editor>> = Rc::new(fe_editor::commands::registry());
}

/// Registers the app commands and binds [`DEFAULT_BINDINGS`].
pub(crate) fn init(cx: &mut App) {
    let mut app = Registry::new();
    app.register("app.quit", "Quit", |cx: &mut &mut App, _: &Actor| {
        cx.quit();
        Ok(Dispatched::new(Effect::Nothing))
    })
    .expect("app command IDs are valid and distinct");
    cx.default_global::<Commands>().app = Rc::new(app);
    cx.on_action(|run: &RunCommand, cx| dispatch_app(&run.0, cx));
    cx.bind_keys(DEFAULT_BINDINGS.iter().map(|binding| {
        let invocation = Invocation::of(binding.command)
            .expect("default bindings name valid command IDs")
            .source(Source::Keyboard);
        KeyBinding::new(binding.keys, RunCommand(invocation), binding.context)
    }));
}

/// Calls `observer` with every invocation the editor and app registries
/// dispatch successfully, after it ran: the seam for a macro recorder, a
/// logger or an agent bridge.
pub fn observe_invocations(cx: &mut App, observer: impl Fn(&Invocation) + 'static) {
    cx.default_global::<Commands>()
        .observers
        .push(Rc::new(observer));
}

fn notify_observers(invocation: &Invocation, cx: &App) {
    if let Some(commands) = cx.try_global::<Commands>() {
        for observer in &commands.observers {
            observer(invocation);
        }
    }
}

/// Runs an app command. Reached by whatever no focused view handled.
fn dispatch_app(invocation: &Invocation, cx: &mut App) {
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
struct ViewHost<'a> {
    cx: &'a mut App,
    page_rows: u32,
    /// The syntax tree, if it matches the text.
    syntax: Option<&'a Syntax>,
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

    /// Runs `invocation` on this editor, in its context. An ID the editor
    /// doesn't know propagates, to the app's commands.
    fn run_command(&mut self, invocation: Invocation, cx: &mut Context<Self>) {
        let invocation = invocation.context(fe_editor::commands::context());
        let registry = EDITOR_COMMANDS.with(Rc::clone);
        let page_rows = self.page_rows();
        let syntax = synced_syntax(&self.syntax, &self.model);
        let mut host = ViewHost {
            cx,
            page_rows,
            syntax,
        };
        let result = registry.dispatch(
            &mut EditorTarget::new(&mut self.model, &mut host),
            &invocation,
        );
        match result {
            Ok(dispatched) => {
                match dispatched.effect {
                    Effect::Changed => self.changed(cx),
                    Effect::Notify => cx.notify(),
                    Effect::Nothing => {}
                }
                notify_observers(&invocation, cx);
            }
            Err(DispatchError::UnknownCommand(_)) => cx.propagate(),
            Err(DispatchError::Failed {
                command,
                error: CommandError(message),
            }) => {
                log::error!("{message}");
                // A failed edit, such as undo, may have moved the carets;
                // a failed save only repaints.
                if registry
                    .get(&command)
                    .is_some_and(|meta| meta.mutates_buffer)
                {
                    self.changed(cx);
                } else {
                    cx.notify();
                }
            }
            Err(error) => log::error!("{error}"),
        }
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
