// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln: an app framework on GPUI that hides the differences between
//! platforms and provides high-level widgets, in GPUI's own idioms. This
//! crate sets an app up ([`app()`], or [`application`] and [`init`] by
//! hand) and re-exports the others; see `docs/framework.md` in kiln.

mod app_builder;
mod app_commands;
mod config;
mod contributions;
mod cx;
mod ext;
pub mod model;
mod setup;
#[cfg(feature = "test-support")]
pub mod testing;

pub use gpui;
pub use kiln_command as command;
pub use kiln_core as core;
pub use kiln_platform as platform;
pub use kiln_shell as shell;
pub use kiln_ui as ui;

/// The function [`app()`] beside the module [`app`](mod@app) of app commands:
/// `kiln::app(..)` sets an app up, and `kiln::app::Quit` is a command.
pub use app_builder::{AppBuilder, AppWindow, Started, app};
pub use contributions::{AnyDockPanel, MakePanel, Module, PanelSpec, StatusSpec};
pub use contributions::{
    CONTRIBUTED_KEYS_PATH, CONTRIBUTED_MENUS_PATH, CommandSpec, Contributions, Keys, MenuSpec,
    RegisterCommand, workspace_handler,
};
pub use cx::{Cx, with_cx};
pub use ext::binding_label_for;
/// serde, for an app's settings type without a serde dependency of its
/// own: `#[derive(Deserialize, Serialize)]` with
/// `#[serde(crate = "kiln::serde", default)]`.
pub use kiln_core::serde;
pub use kiln_core::{SettingsType, settings::SettingsEvent};
/// Makes a fn a command, with `key`, `menu`, `order`, `id` and `context`
/// optional:
///
/// ```
/// use kiln::{Cx, shell::Workspace};
///
/// /// New Note
/// #[kiln::command(key = "secondary-n", menu = "File")]
/// fn new_note(workspace: &mut Workspace, cx: &mut Cx<Workspace>) {
///     // Runs on the workspace of the window the command is dispatched in.
///     # let _ = (workspace, cx);
/// }
/// ```
///
/// - The fn stays as it is, and handles the command, from its keys, its
///   menu item, the palette or [`prelude::WindowExt::run_command`], on the
///   workspace of the window it's dispatched in, once the dispatch is over,
///   seeing its invocation ([`current_invocation`]); the invocation
///   observers hear of each run once ([`workspace_handler`]).
/// - `NewNote`, the fn's name in PascalCase, is the command's type, with
///   the fn's visibility: a unit-struct GPUI action written as
///   [`commands!`] writes one, so `NewNote::id()` is its ID and
///   `.on_action(..)` handles it in a view first.
/// - Its ID is `<crate>.new_note`, `<crate>` being `CARGO_CRATE_NAME`, or
///   `id = "workbench.save_all"`'s, whose last segment comes back from the
///   type it names (`SaveAll`).
/// - Its title is the fn's first doc line: a fn with none doesn't compile.
/// - `key = "secondary-n"` gives it default keys, `secondary` being cmd on
///   macOS and ctrl elsewhere, under the app's keymap and the user's;
///   `context = "editor"` limits it, and its keys, to a key context; and
///   `menu = "File"` puts its item in that menu, after the menu table's,
///   by `order = n` (`0` unless given), then by file and line.
///
/// The macro records the command with `inventory`. [`app()`] adds the
/// records compiled into the app's own binary, example or integration test
/// ([`Contributions::add_recorded`]); a library crate's go in through its
/// [`module!`], which the app passes to [`AppBuilder::module`], as a
/// library crate's would otherwise go missing when nothing referenced it.
/// In a debug build, [`app()`] warns of a library crate whose records are
/// linked in but no `module!` claimed, naming it.
///
/// It expands to the fn, then:
///
/// ```ignore
/// /// New Note
/// pub struct NewNote; // a GPUI action named `<crate>::NewNote`
/// fn register(cx: &mut App, info: CommandInfo) { /* NewNote, and new_note as its handler */ }
/// inventory::submit!(CommandRecord {
///     id: "<crate>.new_note", title: "New Note", keys: Some("secondary-n"),
///     menu: Some("File"), file: file!(), line: line!(), register, ..
/// });
/// ```
///
/// which is, by hand, in a crate named `notes`:
///
/// ```
/// use kiln::{CommandSpec, Contributions, Cx, gpui::App, shell::Workspace};
///
/// mod notes {
///     kiln::commands!(notes, [
///         /// New Note
///         NewNote,
///     ]);
/// }
///
/// fn new_note(workspace: &mut Workspace, cx: &mut Cx<Workspace>) {
///     # let _ = (workspace, cx);
/// }
///
/// fn module(cx: &mut App) {
///     Contributions::add_command(
///         cx,
///         CommandSpec::new::<notes::NewNote>("New Note")
///             .keys("secondary-n")
///             .menu("File", 0),
///     );
///     cx.on_action(kiln::workspace_handler::<notes::NewNote>(new_note));
/// }
/// # let _: kiln::Module = module;
/// ```
///
/// the macro's items also placed in their menu by file and line
/// ([`MenuSpec::declared`]).
///
/// A fn without a doc line doesn't compile:
///
/// ```compile_fail
/// use kiln::{Cx, shell::Workspace};
///
/// #[kiln::command]
/// fn new_note(_: &mut Workspace, _: &mut Cx<Workspace>) {}
/// ```
///
/// nor does an argument it doesn't know, the error on its name:
///
/// ```compile_fail
/// use kiln::{Cx, shell::Workspace};
///
/// /// New Note
/// #[kiln::command(shortcut = "secondary-n")]
/// fn new_note(_: &mut Workspace, _: &mut Cx<Workspace>) {}
/// ```
///
/// nor a fn whose signature isn't a handler's, the error on its name:
///
/// ```compile_fail,E0061
/// use kiln::shell::Workspace;
///
/// /// New Note
/// #[kiln::command]
/// fn new_note(_: &mut Workspace) {}
/// ```
pub use kiln_macros::command;
pub use kiln_platform::WindowSpec;
pub use kiln_platform::config::{ConfigDiagnostics, GlobalSettings};
/// A window's root view, around the app's: the chrome the platform wants
/// and the per-window setup. See [`kiln_shell::Root`].
pub use kiln_shell::Root;
pub use model::provide;
pub use setup::{AppInfo, Config, application, catalog, init, window_options};

/// The extension traits that put kiln's calls on GPUI's own types:
/// `use kiln::prelude::*;` then `cx.theme()`, `cx.settings::<S>()`,
/// `cx.set_theme_mode(..)`, `cx.binding_label(..)` or
/// `window.run_command(..)`; and [`Command`], for a typed command's ID:
/// `NewNote::id()`.
pub mod prelude {
    pub use crate::ext::{AppExt, WindowExt};
    pub use kiln_platform::command_actions::Command;
    pub use kiln_ui::ActiveTheme;
}

// Commands: `kiln::commands!` declares typed commands (GPUI actions with a
// title and context), and these dispatch and record them.
pub use kiln_platform::command_actions::{
    Command, CommandInfo, current_invocation, dispatch_invocation, register_command,
};
pub use kiln_platform::commands;

/// Declares a library crate's [`Module`], `$name`, which adds the commands
/// its `#[kiln::command]`s declared: `kiln::module!(pub fn module);`, then
/// `kiln::app(..).module(notes::module)` in the app. An app's own binary
/// needs none: [`app()`] adds its commands itself.
///
/// ```
/// kiln::module!(pub fn module);
///
/// let module: kiln::Module = module;
/// # let _ = module;
/// ```
#[macro_export]
macro_rules! module {
    ($(#[$attr:meta])* $vis:vis fn $name:ident) => {
        $(#[$attr])*
        $vis fn $name(cx: &mut $crate::gpui::App) {
            $crate::__private::add_crate_commands(cx, ::std::env!("CARGO_CRATE_NAME"));
        }
    };
}

/// `#[kiln::command]`'s command type: a unit-struct GPUI action named
/// `$action`, documented by `$doc`, written as [`commands!`] writes one.
#[doc(hidden)]
#[macro_export]
macro_rules! __command {
    ($vis:vis $name:ident, $action:expr, [$($doc:literal),*]) => {
        $(#[doc = $doc])*
        #[derive(::std::clone::Clone, ::std::cmp::PartialEq, ::std::default::Default, ::std::fmt::Debug)]
        $vis struct $name;

        impl $crate::gpui::Action for $name {
            fn name(&self) -> &'static str {
                <Self as $crate::gpui::Action>::name_for_type()
            }

            fn name_for_type() -> &'static str
            where
                Self: Sized,
            {
                $action
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
                DOC.get_or_init(|| {
                    $crate::platform::command_actions::documentation_from_doc(&[$($doc),*])
                })
                .as_deref()
            }
        }

        impl $crate::Command for $name {}

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
    };
}

/// What kiln's macros expand to names: not for use by hand.
#[doc(hidden)]
pub mod __private {
    pub use crate::contributions::{
        CommandRecord, RunRecorded, add_crate_commands, register_recorded as register_command,
    };
    pub use inventory;
}

/// The app commands, as typed actions: `app.quit` is `app::Quit`. `init`
/// registers them and runs them app-wide; bind them as `app.quit`.
pub mod app {
    kiln_platform::commands!(
        app,
        [
            /// Quit
            Quit,
            /// Hide
            Hide,
            /// Hide Others
            HideOthers,
            /// Show All
            ShowAll,
        ]
    );
}

/// The window commands, as typed actions on the active window:
/// `window.close` is `window::Close`.
pub mod window {
    kiln_platform::commands!(
        window,
        [
            /// Minimize
            Minimize,
            /// Zoom
            Zoom,
            /// Toggle Full Screen
            ToggleFullscreen,
            /// Close Window
            Close,
        ]
    );
}
