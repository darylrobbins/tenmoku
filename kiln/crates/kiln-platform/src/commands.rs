// SPDX-License-Identifier: LicenseRef-Proprietary

//! GPUI's side of `kiln-command`. Every key binding, menu item and status
//! item dispatches one GPUI action, [`RunCommand`], carrying an
//! [`Invocation`]. A focused view that knows the command runs it; anything
//! else propagates to the app's registry, which [`init`] owns: `app.quit`,
//! `window.close` and the like, and whatever a host adds with
//! [`register_app_commands`].

use std::rc::Rc;

use gpui::{Action, AnyWindowHandle, App, Global, SharedString, Window};
use kiln_command::{
    Actor, CommandId, CommandMeta, CommandTarget, Dispatched, Effect, Invocation, Registry, Value,
};
use kiln_core::{CatalogError, CommandCatalog};

/// Runs a command: the one GPUI action every binding dispatches.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = kiln, no_json)]
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

type AppCommand = fn(&mut App);

/// The app commands, in no context: `app.quit`, `app.hide` (titled for
/// `app_name`), `app.hide_others`, `app.show_all`, and `window.minimize`,
/// `window.zoom`, `window.toggle_fullscreen` and `window.close`, which act
/// on the active window and do nothing without one. [`init`] installs
/// them; this builds the same set for checking keymaps without an app.
pub fn app_commands(app_name: &str) -> Registry<AppCommands> {
    let mut app = Registry::new();
    let hide = format!("Hide {app_name}");
    let commands: [(&str, &str, AppCommand); 8] = [
        ("app.quit", "Quit", quit),
        ("app.hide", &hide, |cx| cx.hide()),
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
        ("window.close", "Close Window", close_window),
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

/// What `app.quit` and `window.close` run instead of quitting and
/// closing outright: a shell's, which asks the windows' items first.
#[derive(Clone)]
struct CloseHandlers {
    quit: Rc<dyn Fn(&mut App)>,
    close_window: Rc<CloseWindow>,
}

/// What `window.close` runs on a window.
type CloseWindow = dyn Fn(AnyWindowHandle, &mut App);

impl Global for CloseHandlers {}

/// Has `app.quit` run `quit`, and `window.close` run `close_window` on the
/// active window, in place of quitting and closing it outright: for a
/// shell that asks the windows' items whether they may close first.
/// `close_window` runs once the dispatch is over, as the window can't be
/// updated until then.
pub fn set_close_handlers(
    cx: &mut App,
    quit: impl Fn(&mut App) + 'static,
    close_window: impl Fn(AnyWindowHandle, &mut App) + 'static,
) {
    cx.set_global(CloseHandlers {
        quit: Rc::new(quit),
        close_window: Rc::new(close_window),
    });
}

/// `app.quit`: the close handlers' quit, else quitting.
fn quit(cx: &mut App) {
    match cx.try_global::<CloseHandlers>().cloned() {
        Some(handlers) => (handlers.quit)(cx),
        None => cx.quit(),
    }
}

/// `window.close`: the close handlers' close on the active window, else
/// closing it.
fn close_window(cx: &mut App) {
    let Some(handlers) = cx.try_global::<CloseHandlers>().cloned() else {
        return on_active_window(cx, |window| window.remove_window());
    };
    if let Some(handle) = target_window(cx) {
        cx.defer(move |cx| (handlers.close_window)(handle, cx));
    }
}

/// The window a window command acts on: the one it was dispatched in, else
/// the active one.
fn target_window(cx: &App) -> Option<AnyWindowHandle> {
    crate::command_actions::dispatch_window(cx).or_else(|| cx.active_window())
}

/// Runs `f` on the window the command was dispatched in, else the active
/// window, if any, once the current dispatch is over: a key or menu item
/// runs its command while GPUI has that window out for the dispatch, so it
/// can't be updated until then.
fn on_active_window(cx: &mut App, f: fn(&mut Window)) {
    if let Some(handle) = target_window(cx) {
        cx.defer(move |cx| {
            handle.update(cx, |_, window, _| f(window)).ok();
        });
    }
}

/// Registers the app commands, titled for `app_name` (`Hide Notes`), and
/// the handler that runs whatever reaches the app with a [`RunCommand`].
/// Called again, it only renames.
pub fn init(cx: &mut App, app_name: impl Into<SharedString>) {
    let first = !cx.has_global::<Commands>();
    cx.default_global::<Commands>().app = Rc::new(app_commands(&app_name.into()));
    if first {
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
}

/// Whether [`init`] has run: from then on, a command can be dispatched.
pub fn initialized(cx: &App) -> bool {
    cx.has_global::<Commands>()
}

/// Runs `run`'s command as its typed command's action, if `run.0` names a
/// registered one ([`crate::command_actions::typed_command`]), through
/// [`crate::command_actions::dispatch_invocation`] in `window`, keeping
/// its context and source, and returns true; false otherwise, running
/// nothing, for the caller to go on with it. For a view's or the app's
/// [`RunCommand`] handler: a raw `RunCommand` naming a typed command, as a
/// test dispatches with [`Window::dispatch_action`], never reaches its
/// action otherwise.
///
/// The action is dispatched once the current dispatch is over, from the
/// window's focused element: the view whose handler calls this is being
/// updated until then, so its own handler of the action couldn't run.
pub fn dispatch_typed(run: &RunCommand, window: &mut Window, cx: &mut App) -> bool {
    let Some(typed) = crate::command_actions::typed_command(&run.0.command, cx) else {
        return false;
    };
    if let Err(message) = typed.build(&run.0.args) {
        log::warn!("{}: {message}", run.0.command);
        return true;
    }
    let (invocation, handle) = (run.0.clone(), window.window_handle());
    cx.defer(move |cx| {
        let dispatched = handle.update(cx, |_, window, cx| {
            crate::command_actions::dispatch_invocation(invocation, window, cx)
        });
        if let Ok(Err(error)) = dispatched {
            log::warn!("{error}");
        }
    });
    true
}

/// Adds a host's own app commands beside `app.*` and `window.*`. Call
/// after [`init`].
pub fn register_app_commands(cx: &mut App, f: impl FnOnce(&mut Registry<AppCommands>)) {
    let commands = cx.default_global::<Commands>();
    let mut registry = std::mem::take(&mut commands.app);
    f(Rc::get_mut(&mut registry).expect("the app registry isn't shared while registering"));
    commands.app = registry;
}

/// The app registry's commands (`app.quit` and the rest, and a host's own),
/// for a palette to list.
pub fn app_command_list(cx: &App) -> Vec<CommandMeta> {
    cx.try_global::<Commands>()
        .map(|commands| commands.app.commands().cloned().collect())
        .unwrap_or_default()
}

/// The app's commands, to check bindings and menu items against: the app
/// registry's, then the typed commands registered so far
/// ([`crate::command_actions`]), whose args are checked by building their
/// action.
pub fn catalog(cx: &App) -> Rc<dyn CommandCatalog> {
    let registry = cx
        .try_global::<Commands>()
        .map(|commands| commands.app.clone())
        .unwrap_or_default();
    Rc::new(AppCatalog {
        registry,
        typed: crate::command_actions::typed_commands(cx),
    })
}

struct AppCatalog {
    registry: Rc<Registry<AppCommands>>,
    typed: Vec<crate::command_actions::TypedCommand>,
}

impl CommandCatalog for AppCatalog {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        match self.registry.check(command, args) {
            Err(CatalogError::UnknownCommand) => {
                let typed = self.typed.iter().find(|typed| &typed.id == command);
                let typed = typed.ok_or(CatalogError::UnknownCommand)?;
                typed.build(args).map(drop).map_err(CatalogError::BadArgs)
            }
            result => result,
        }
    }
}

/// Calls `observer` with every invocation the app's registry, and any
/// view that reports through [`notify_observers`], dispatches
/// successfully, after it ran: the seam for a macro recorder, a logger or
/// an agent bridge.
pub fn observe_invocations(cx: &mut App, observer: impl Fn(&Invocation) + 'static) {
    cx.default_global::<Commands>()
        .observers
        .push(Rc::new(observer));
}

/// Tells the observers `invocation` ran: for a view that dispatched it
/// through its own registry.
pub fn notify_observers(invocation: &Invocation, cx: &App) {
    if let Some(commands) = cx.try_global::<Commands>() {
        for observer in &commands.observers {
            observer(invocation);
        }
    }
}

/// Lets `invocation`, which a focused view's registry doesn't know, go on
/// to the app's, still in the view's context: GPUI propagates the original
/// [`RunCommand`], whose context is the dispatch's. Call from the view's
/// `RunCommand` handler.
pub fn propagate_with_context(invocation: Invocation, cx: &mut App) {
    cx.default_global::<Commands>().propagated = Some(invocation);
    // In case no app handler takes it.
    cx.defer(|cx| cx.default_global::<Commands>().propagated = None);
    cx.propagate();
}

/// Runs an app command: what no focused view handled, or a menu item
/// chosen with no window open.
pub fn dispatch_app(invocation: &Invocation, cx: &mut App) {
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
