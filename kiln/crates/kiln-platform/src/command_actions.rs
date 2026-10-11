// SPDX-License-Identifier: LicenseRef-Proprietary

//! Typed commands: each is a real [`gpui::Action`], handled the GPUI way
//! (`.on_action(cx.listener(Self::save))`), with the metadata a command
//! needs beyond that (a title, the context it applies in) so menus, the
//! palette, keymap checks and agents can list it. Every dispatch, from a
//! key, a menu, the palette or an agent, starts from an [`Invocation`]:
//! [`dispatch_invocation`] builds the action from it, and handlers can read
//! it back with [`current_invocation`].
//!
//! ```ignore
//! kiln::commands!(notes, [
//!     /// New Note
//!     NewNote,
//!     /// Delete Note
//!     #[command(context = "notes_list")]
//!     DeleteNote,
//! ]);
//! // notes::register(cx) records them; `.on_action(cx.listener(Self::new_note))`
//! // handles one; Invocation::of("notes.new_note") dispatches it.
//! ```
//!
//! A command's ID is its action's GPUI name with `::` for `.` and the type
//! in snake case: `notes::NewNote` is `notes.new_note`, which
//! [`Command::id`] gives without spelling it: `NewNote::id()`. The macro
//! implements `gpui::Action` itself, through kiln's re-export of GPUI, so
//! an app using it needs no `gpui` dependency of its own.

use std::collections::HashMap;

use gpui::{Action, AnyWindowHandle, App, Global, Window};
use kiln_command::{CommandId, ContextPredicate, DispatchError, Invocation, Value};

use crate::commands::{RunCommand, notify_observers};

/// A typed command: an action whose GPUI name has a namespace, so it has a
/// kiln [`CommandId`]. [`commands!`] implements it for the actions it
/// declares; an action with args, declared with `#[derive(gpui::Action)]`
/// and `#[action(namespace = ..)]`, can opt in with an empty
/// `impl Command for OpenNote {}`.
pub trait Command: Action {
    /// The command's ID: `notes::NewNote`'s is `notes.new_note`.
    ///
    /// # Panics
    ///
    /// If the action's GPUI name has no namespace.
    fn id() -> CommandId
    where
        Self: Sized,
    {
        let name = Self::name_for_type();
        command_id(name).unwrap_or_else(|| panic!("{name} needs a namespace to be a command"))
    }
}

/// What [`register_command`] records about a command besides its action.
#[derive(Clone, Debug, Default)]
pub struct CommandInfo {
    /// What menus and the palette call it.
    pub title: String,
    /// Where it applies, as a key context predicate (`notes_list`); `None`
    /// is everywhere.
    pub context: Option<&'static str>,
}

/// One typed command.
#[derive(Clone)]
pub struct TypedCommand {
    pub id: CommandId,
    /// GPUI's name for the action: `notes::NewNote`.
    pub action_name: &'static str,
    pub title: String,
    pub context: Option<ContextPredicate>,
    build: fn(serde_json::Value) -> gpui::Result<Box<dyn Action>>,
}

impl TypedCommand {
    /// The action for `args` (`Null` for none), or why they don't fit it.
    pub fn build(&self, args: &Value) -> Result<Box<dyn Action>, String> {
        (self.build)(args.clone()).map_err(|error| error.to_string())
    }
}

#[derive(Default)]
struct TypedCommands {
    by_id: HashMap<CommandId, TypedCommand>,
    /// The invocation being dispatched, while its handler runs.
    current: Option<Invocation>,
    /// The window it's dispatched in.
    window: Option<AnyWindowHandle>,
}

impl Global for TypedCommands {}

/// Records `A` as a command, under the ID its GPUI name gives it. A second
/// registration of the same ID replaces the first.
pub fn register_command<A: Action>(cx: &mut App, info: CommandInfo) {
    let action_name = A::name_for_type();
    let id = command_id(action_name)
        .unwrap_or_else(|| panic!("{action_name} needs a namespace to be a command"));
    let context = info.context.map(|context| {
        ContextPredicate::parse(context)
            .unwrap_or_else(|error| panic!("{action_name}'s context {context:?}: {error}"))
    });
    let command = TypedCommand {
        id: id.clone(),
        action_name,
        title: info.title,
        context,
        build: A::build,
    };
    cx.default_global::<TypedCommands>()
        .by_id
        .insert(id, command);
}

/// Every typed command registered, by ID.
pub fn typed_commands(cx: &App) -> Vec<TypedCommand> {
    let mut commands: Vec<_> = cx
        .try_global::<TypedCommands>()
        .map(|typed| typed.by_id.values().cloned().collect())
        .unwrap_or_default();
    commands.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    commands
}

/// The typed command `id`, if one is registered.
pub fn typed_command(id: &CommandId, cx: &App) -> Option<TypedCommand> {
    cx.try_global::<TypedCommands>()?.by_id.get(id).cloned()
}

/// The command ID for GPUI's action name `name`: `notes::NewNote` is
/// `notes.new_note`. `None` without a namespace.
pub fn command_id(name: &str) -> Option<CommandId> {
    let (namespace, type_name) = name.rsplit_once("::")?;
    let id = format!("{}.{}", namespace.replace("::", "."), snake_case(type_name));
    CommandId::parse(&id).ok()
}

fn snake_case(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (ix, &ch) in chars.iter().enumerate() {
        if ch.is_uppercase() {
            let after_lower =
                ix > 0 && (chars[ix - 1].is_lowercase() || chars[ix - 1].is_ascii_digit());
            let before_lower = ix > 0
                && chars[ix - 1].is_uppercase()
                && chars.get(ix + 1).is_some_and(|next| next.is_lowercase());
            if after_lower || before_lower {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// The invocation being dispatched, while its typed command's handler
/// runs: who asked (`actor`), from where (`source`), and in what context.
pub fn current_invocation(cx: &App) -> Option<Invocation> {
    cx.try_global::<TypedCommands>()?.current.clone()
}

/// Dispatches `invocation` in `window`, from its focused element: as its
/// typed command's action, built from its args, if one is registered; as
/// a [`RunCommand`] otherwise, for commands that live in a registry
/// rather than as actions. With an element focused, and drawn in the
/// window's last frame, it runs now; otherwise once the current dispatch
/// is over, from the window's root (as [`Window::dispatch_action`] does).
/// Observers hear of a typed command that some element handles; a
/// registry's handler tells them itself.
///
/// Focus can rest on an element that's no longer drawn, such as one in a
/// page that was hidden: GPUI keeps it focused, and drops an action sent
/// to it. Sent from the root instead, it reaches the app's global
/// handlers, such as its command registry's; and the window is
/// [`crate::focus::watch`]ed, so once a frame is drawn focus moves to a
/// view that is.
pub fn dispatch_invocation(
    invocation: Invocation,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), DispatchError> {
    // Run for every key a binding matches, so it clones no more than it
    // must: the invocation once for a `RunCommand` (the action and the
    // handlers' current invocation each hold one) and not at all for a
    // typed command, unless deferred.
    let action: Box<dyn Action> = match typed_action(&invocation, cx) {
        None => Box::new(RunCommand(invocation.clone())),
        Some(Ok(action)) => action,
        Some(Err(message)) => {
            return Err(DispatchError::BadArgs {
                command: invocation.command,
                message,
            });
        }
    };
    let typed = !action.as_any().is::<RunCommand>();
    let handled = typed && window.is_action_available(action.as_ref(), cx);
    crate::focus::watch(window, cx);
    match window
        .focused(cx)
        .filter(|focus| crate::focus::is_drawn_for(focus, action.as_ref(), window, cx))
    {
        Some(focus) => {
            let previous = set_current(Some(invocation), cx);
            let previous_window = set_window(Some(window.window_handle()), cx);
            focus.dispatch_action(action.as_ref(), window, cx);
            // A nested dispatch puts back the invocation it replaced, so
            // this is the one set above.
            let invocation = set_current(previous, cx);
            set_window(previous_window, cx);
            if handled && let Some(invocation) = invocation {
                notify_observers(&invocation, cx);
            }
        }
        // The window dispatches later, so the invocation is set around
        // that: deferred work runs in order.
        None => {
            let observed = handled.then(|| invocation.clone());
            let handle = window.window_handle();
            cx.defer(move |cx| {
                set_current(Some(invocation), cx);
                set_window(Some(handle), cx);
            });
            window.dispatch_action(action, cx);
            cx.defer(|cx| {
                set_current(None, cx);
                set_window(None, cx);
            });
            if let Some(invocation) = observed {
                notify_observers(&invocation, cx);
            }
        }
    }
    Ok(())
}

/// The action for `invocation`'s typed command, built from its args, or
/// why they don't fit it; `None` if no typed command has its ID. Unlike
/// [`typed_command`], it clones nothing but the args.
fn typed_action(invocation: &Invocation, cx: &App) -> Option<Result<Box<dyn Action>, String>> {
    let command = cx
        .try_global::<TypedCommands>()?
        .by_id
        .get(&invocation.command)?;
    Some(command.build(&invocation.args))
}

/// Sets the window a dispatch is in, returning the one it replaces.
fn set_window(window: Option<AnyWindowHandle>, cx: &mut App) -> Option<AnyWindowHandle> {
    std::mem::replace(&mut cx.default_global::<TypedCommands>().window, window)
}

/// The window the command being dispatched was dispatched in, from a key
/// or [`dispatch_invocation`]: the one a window command acts on, which may
/// not be the platform's active window (an X server without a window
/// manager has none).
pub fn dispatch_window(cx: &App) -> Option<AnyWindowHandle> {
    cx.try_global::<TypedCommands>()?.window
}

/// Sets the invocation handlers see, returning the one it replaces.
fn set_current(invocation: Option<Invocation>, cx: &mut App) -> Option<Invocation> {
    std::mem::replace(
        &mut cx.default_global::<TypedCommands>().current,
        invocation,
    )
}

/// Declares typed commands: unit-struct GPUI actions in namespace `$ns`,
/// each titled by its first doc line and optionally limited to a context
/// with `#[command(context = "...")]`, plus `pub fn register(cx)`, which
/// records them (see [`register_command`]). Each implements [`Command`],
/// so `NewNote::id()` is its ID.
///
/// It implements `gpui::Action` as `#[derive(gpui::Action)]` would, but
/// names GPUI only through `$crate`, so it works with nothing but kiln as
/// a dependency: GPUI's derive writes bare `gpui::` paths, which resolve
/// only where the `gpui` crate is in scope under that name. Each action's
/// documentation, for GPUI's action registry, is its doc lines.
#[macro_export]
macro_rules! commands {
    ($ns:ident, [ $( $(#[doc = $doc:literal])* $(#[command(context = $context:literal)])? $name:ident ),* $(,)? ]) => {
        $(
            $(#[doc = $doc])*
            #[derive(::std::clone::Clone, ::std::cmp::PartialEq, ::std::default::Default, ::std::fmt::Debug)]
            pub struct $name;

            impl $crate::gpui::Action for $name {
                fn name(&self) -> &'static str {
                    <Self as $crate::gpui::Action>::name_for_type()
                }

                fn name_for_type() -> &'static str
                where
                    Self: Sized,
                {
                    ::std::concat!(::std::stringify!($ns), "::", ::std::stringify!($name))
                }

                fn partial_eq(&self, action: &dyn $crate::gpui::Action) -> bool {
                    action.as_any().downcast_ref::<Self>().is_some_and(|a| self == a)
                }

                fn boxed_clone(&self) -> ::std::boxed::Box<dyn $crate::gpui::Action> {
                    ::std::boxed::Box::new(self.clone())
                }

                fn build(
                    _: $crate::gpui::private::serde_json::Value,
                ) -> $crate::gpui::Result<::std::boxed::Box<dyn $crate::gpui::Action>> {
                    ::std::result::Result::Ok(::std::boxed::Box::new(Self))
                }

                fn documentation() -> ::std::option::Option<&'static str>
                where
                    Self: Sized,
                {
                    static DOC: ::std::sync::OnceLock<::std::option::Option<::std::string::String>> =
                        ::std::sync::OnceLock::new();
                    DOC.get_or_init(|| $crate::command_actions::documentation_from_doc(&[$($doc),*]))
                        .as_deref()
                }
            }

            impl $crate::command_actions::Command for $name {}

            // GPUI's action registry finds its actions through `inventory`,
            // as its derive arranges.
            const _: () = {
                fn builder() -> $crate::gpui::MacroActionData {
                    $crate::gpui::MacroActionData {
                        name: <$name as $crate::gpui::Action>::name_for_type(),
                        type_id: ::std::any::TypeId::of::<$name>(),
                        build: <$name as $crate::gpui::Action>::build,
                        json_schema: <$name as $crate::gpui::Action>::action_json_schema,
                        deprecated_aliases: <$name as $crate::gpui::Action>::deprecated_aliases(),
                        deprecation_message: <$name as $crate::gpui::Action>::deprecation_message(),
                        documentation: <$name as $crate::gpui::Action>::documentation(),
                    }
                }
                $crate::gpui::private::inventory::submit! {
                    $crate::gpui::MacroActionBuilder(builder)
                }
            };
        )*

        /// Records this module's commands' titles and contexts.
        pub fn register(cx: &mut $crate::gpui::App) {
            $(
                $crate::command_actions::register_command::<$name>(
                    cx,
                    $crate::command_actions::CommandInfo {
                        title: $crate::command_actions::title_from_doc(&[$($doc),*], ::std::stringify!($name)),
                        context: ::std::option::Option::None $(.or(::std::option::Option::Some($context)))?,
                    },
                );
            )*
        }
    };
}

/// A title from doc lines: the first non-empty one, trimmed, or the type's
/// name if there's none.
#[doc(hidden)]
pub fn title_from_doc(lines: &[&str], name: &str) -> String {
    lines
        .iter()
        .map(|line| line.trim())
        .find(|line| !line.is_empty())
        .unwrap_or(name)
        .to_owned()
}

/// An action's documentation from doc lines, as GPUI's derive writes it:
/// each line trimmed, joined by newlines, trimmed; `None` without any.
#[doc(hidden)]
pub fn documentation_from_doc(lines: &[&str]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let joined: String = lines
        .iter()
        .map(|line| format!("{}\n", line.trim()))
        .collect();
    Some(joined.trim().to_owned())
}
