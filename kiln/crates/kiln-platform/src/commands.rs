// SPDX-License-Identifier: LicenseRef-Proprietary

//! GPUI's side of `kiln-command`. Every key binding, menu item and status
//! item dispatches one GPUI action, [`RunCommand`], carrying an
//! [`Invocation`]. A focused view that knows the command runs it; anything
//! else propagates to the app's registry, which [`init`] owns: `app.quit`,
//! `window.close` and the like, and whatever a host adds with
//! [`register_app_commands`].

use std::rc::Rc;

use gpui::{Action, App, Global, SharedString, Window};
use kiln_command::{
    Actor, CommandId, CommandTarget, Dispatched, Effect, Invocation, Registry, Value,
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
        ("app.quit", "Quit", |cx| cx.quit()),
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
        ("window.close", "Close Window", |cx| {
            on_active_window(cx, |window| window.remove_window())
        }),
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

/// Adds a host's own app commands beside `app.*` and `window.*`. Call
/// after [`init`].
pub fn register_app_commands(cx: &mut App, f: impl FnOnce(&mut Registry<AppCommands>)) {
    let commands = cx.default_global::<Commands>();
    let mut registry = std::mem::take(&mut commands.app);
    f(Rc::get_mut(&mut registry).expect("the app registry isn't shared while registering"));
    commands.app = registry;
}

/// The app registry's commands, to check bindings and menu items against.
pub fn catalog(cx: &App) -> Rc<dyn CommandCatalog> {
    let registry = cx
        .try_global::<Commands>()
        .map(|commands| commands.app.clone())
        .unwrap_or_default();
    Rc::new(AppCatalog(registry))
}

struct AppCatalog(Rc<Registry<AppCommands>>);

impl CommandCatalog for AppCatalog {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        self.0.check(command, args)
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
