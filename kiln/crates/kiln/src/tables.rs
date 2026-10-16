// SPDX-License-Identifier: LicenseRef-Proprietary

//! Typed keymap and menu tables: [`keymap!`](crate::keymap!) and
//! [`menus!`](crate::menus!) name commands by type, so a typo is a compile
//! error where a TOML table's is a warning at start.
//!
//! ```
//! # mod notes { kiln::commands!(notes, [
//! #     /// New Note
//! #     NewNote,
//! #     /// Rename Note
//! #     RenameNote,
//! # ]); }
//! # use notes::{NewNote, RenameNote};
//! let keys = kiln::keymap! {
//!     "secondary-shift-n" => NewNote,
//!     "f2" => RenameNote in "note",
//!     { macos = "cmd-]", default = "ctrl-]" } => kiln::app::Quit,
//! };
//! let menus = kiln::menus! {
//!     "File" => [NewNote, Separator, kiln::app::Quit],
//! };
//! # let _ = (keys, menus);
//! ```
//!
//! A misspelt command type doesn't compile:
//!
//! ```compile_fail
//! # mod notes { kiln::commands!(notes, [
//! #     /// New Note
//! #     NewNote,
//! # ]); }
//! # use notes::NewNote;
//! let menus = kiln::menus! { "File" => [NewNotes] };
//! ```
//!
//! nor does one in a keymap:
//!
//! ```compile_fail
//! # mod notes { kiln::commands!(notes, [
//! #     /// New Note
//! #     NewNote,
//! # ]); }
//! # use notes::NewNote;
//! let keys = kiln::keymap! { "secondary-n" => NewNotes };
//! ```

use kiln_command::CommandId;
use kiln_core::{CommandCatalog, Diagnostic, KeymapLayer, Os};

use crate::{Command, Keys, contributions::keymap_layer};

/// One key binding of a [`KeyTable`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyBinding {
    pub command: CommandId,
    pub keys: Keys,
    /// A key context predicate; `None` binds everywhere.
    pub context: Option<&'static str>,
}

/// What [`keymap!`](crate::keymap!) builds: the app's default keys, which
/// [`crate::AppBuilder::keys`] puts in place of the commands' attribute
/// ones.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyTable {
    pub(crate) bindings: Vec<KeyBinding>,
}

impl KeyTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `keys` to `A`, in `context` if given.
    pub fn bind<A: Command>(&mut self, keys: impl Into<Keys>, context: Option<&'static str>) {
        self.bindings.push(KeyBinding {
            command: A::id(),
            keys: keys.into(),
            context,
        });
    }

    pub fn bindings(&self) -> &[KeyBinding] {
        &self.bindings
    }

    /// The table as a keymap layer on `os`, and its diagnostics, as the app
    /// ships it: `secondary` is cmd on macOS and ctrl elsewhere, and a
    /// `{ macos = .., linux = .. }` entry is the one for `os`, else
    /// `default`. At runtime `os` is the platform the app runs on; naming
    /// it lets a test check another's, from any host.
    pub fn resolve(&self, os: Os, catalog: &dyn CommandCatalog) -> (KeymapLayer, Vec<Diagnostic>) {
        keymap_layer(&self.bindings, os, catalog)
    }
}

/// An item of a [`MenuTable`]'s menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableItem {
    /// A command's item. The title is the one the command was registered
    /// with, else the first line of its docs.
    Command {
        command: CommandId,
        title: String,
    },
    Separator,
}

/// A menu of a [`MenuTable`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableMenu {
    pub name: &'static str,
    pub items: Vec<TableItem>,
}

impl TableMenu {
    pub fn command<A: Command>(&mut self) {
        let name = A::name_for_type();
        let title = A::documentation()
            .and_then(|docs| docs.lines().next().map(|line| line.trim().to_owned()))
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| name.rsplit("::").next().unwrap_or(name).to_owned());
        self.items.push(TableItem::Command {
            command: A::id(),
            title,
        });
    }

    pub fn separator(&mut self) {
        self.items.push(TableItem::Separator);
    }
}

/// What [`menus!`](crate::menus!) builds: the app's whole menu bar, which
/// [`crate::AppBuilder::menu_bar`] installs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MenuTable {
    pub(crate) menus: Vec<TableMenu>,
}

impl MenuTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts the menu `name`, after those before it.
    pub fn menu(&mut self, name: &'static str) -> &mut TableMenu {
        self.menus.push(TableMenu {
            name,
            items: Vec::new(),
        });
        self.menus.last_mut().expect("just pushed")
    }

    pub fn menus(&self) -> &[TableMenu] {
        &self.menus
    }
}

/// A keymap that names its commands by type, for
/// [`AppBuilder::keys`](crate::AppBuilder::keys): a typo is a compile
/// error, not a warning at start.
///
/// ```ignore
/// kiln::keymap! {
///     "secondary-shift-n" => NewNote,
///     "f2" => RenameNote in "note",
///     { macos = "cmd-]", default = "ctrl-]" } => kiln::app::Quit,
/// }
/// ```
///
/// Each entry is keys, `=>`, a command type, and optionally `in` a key
/// context. The keys are a string, as a keymap file's `keys` is, or
/// `{ macos = "..", default = ".." }` to give them by platform. The table
/// is, in a keymap file:
///
/// ```toml
/// [[bindings]]
/// keys = "secondary-shift-n"
/// command = "notes.new_note"
///
/// [[bindings]]
/// keys = "f2"
/// command = "notes.rename_note"
/// context = "note"
///
/// [[bindings]]
/// keys = { macos = "cmd-]", default = "ctrl-]" }
/// command = "app.quit"
/// ```
///
/// It is checked as the file is, at init: an unknown context or key is a
/// diagnostic.
#[macro_export]
macro_rules! keymap {
    ($($entries:tt)*) => {{
        let mut table = $crate::KeyTable::new();
        $crate::__keymap!(table; $($entries)*);
        table
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __keymap {
    (@bind $t:ident $keys:tt [$($command:tt)+] $context:expr) => {
        $t.bind::<$($command)+>($crate::__keys!($keys), $context);
    };
    (@cmd $t:ident $keys:tt [$($command:tt)+] in $context:literal $(, $($rest:tt)*)?) => {
        $crate::__keymap!(@bind $t $keys [$($command)+] ::std::option::Option::Some($context));
        $($crate::__keymap!($t; $($rest)*);)?
    };
    (@cmd $t:ident $keys:tt [$($command:tt)+] , $($rest:tt)*) => {
        $crate::__keymap!(@bind $t $keys [$($command)+] ::std::option::Option::None);
        $crate::__keymap!($t; $($rest)*);
    };
    (@cmd $t:ident $keys:tt [$($command:tt)+]) => {
        $crate::__keymap!(@bind $t $keys [$($command)+] ::std::option::Option::None);
    };
    (@cmd $t:ident $keys:tt [$($command:tt)*] $next:tt $($rest:tt)*) => {
        $crate::__keymap!(@cmd $t $keys [$($command)* $next] $($rest)*);
    };
    ($t:ident;) => {};
    ($t:ident; $keys:tt => $($rest:tt)+) => {
        $crate::__keymap!(@cmd $t $keys [] $($rest)+);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __keys {
    ({ $($os:ident = $keys:literal),+ $(,)? }) => {
        $crate::Keys::ByPlatform(::std::vec![$((
            ::std::borrow::Cow::Borrowed(::std::stringify!($os)),
            ::std::borrow::Cow::Borrowed($keys),
        )),+])
    };
    ($keys:literal) => {
        $crate::Keys::from($keys)
    };
}

/// A menu bar that names its commands by type, for
/// [`AppBuilder::menu_bar`](crate::AppBuilder::menu_bar): a typo is a
/// compile error, not a warning at start.
///
/// ```ignore
/// kiln::menus! {
///     "File" => [NewNote, Separator, kiln::app::Quit],
///     "Edit" => [RenameNote],
/// }
/// ```
///
/// Each menu is its name, `=>`, and its items in brackets: a command type,
/// whose item is titled as the command is (the title it was registered
/// with, else the first line of its docs), or `Separator`. The table is,
/// in a menu file:
///
/// ```toml
/// [[menus]]
/// name = "File"
///
/// [[menus.items]]
/// title = "New Note"
/// command = "notes.new_note"
///
/// [[menus.items]]
/// separator = true
///
/// [[menus.items]]
/// title = "Quit"
/// command = "app.quit"
/// ```
///
/// It is checked as the file is, at init: a command kiln doesn't know is
/// left out of the menu, with a warning; a menu named twice, or a command
/// twice in one menu, is an error, and the table is left out.
#[macro_export]
macro_rules! menus {
    ($($name:literal => [$($items:tt)*]),* $(,)?) => {{
        let mut table = $crate::MenuTable::new();
        $({
            let menu = table.menu($name);
            $crate::__menu_items!(menu; $($items)*);
        })*
        table
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __menu_items {
    (@cmd $m:ident [$($command:tt)+] , $($rest:tt)*) => {
        $m.command::<$($command)+>();
        $crate::__menu_items!($m; $($rest)*);
    };
    (@cmd $m:ident [$($command:tt)+]) => {
        $m.command::<$($command)+>();
    };
    (@cmd $m:ident [$($command:tt)*] $next:tt $($rest:tt)*) => {
        $crate::__menu_items!(@cmd $m [$($command)* $next] $($rest)*);
    };
    ($m:ident;) => {};
    ($m:ident; Separator $(, $($rest:tt)*)?) => {
        $m.separator();
        $($crate::__menu_items!($m; $($rest)*);)?
    };
    ($m:ident; $($rest:tt)+) => {
        $crate::__menu_items!(@cmd $m [] $($rest)+);
    };
}
