// SPDX-License-Identifier: LicenseRef-Proprietary

//! Kiln's app shell: the root view that draws the window's chrome
//! ([`Root`]), and the chrome's pieces for a host that lays out its own
//! root: the unified title bar ([`TitleBar`]), the in-window menu bar
//! ([`MenuBarView`]) and the frame drawn under client-side decorations
//! ([`window_frame`]). Each reads what the platform does natively from
//! [`Capabilities`], or GPUI's window decorations, and its colours from
//! the active theme.
//!
//! The shell's leaf modules, which a host can use before it adopts the
//! rest: the status bar's items ([`status`]), a picker palette over a
//! delegate ([`picker`]), and a background writer for saved state
//! ([`persist`]).

pub mod menu_bar;
pub mod persist;
pub mod picker;
mod root;
pub mod status;
mod title_bar;
pub mod window_frame;

use gpui::App;
use kiln_command::{CommandId, Value};
use kiln_core::{CatalogError, CommandCatalog};
use kiln_platform::Capabilities;

pub use menu_bar::MenuBarView;
pub use picker::{Picker, PickerDelegate};
pub use root::Root;
pub use status::{StatusItem, StatusItems, StatusSide};
pub use title_bar::TitleBar;

/// Sets up what the shell draws for the platform: where it has no native
/// alerts, `window.prompt` shows kiln-ui's in-window prompt, themed and
/// wrapped, instead of GPUI's plain fallback.
pub fn init(cx: &mut App) {
    if !Capabilities::current().native_prompts {
        cx.set_prompt_builder(kiln_ui::prompt_renderer);
    }
}

/// Records the shell's commands, such as `menu_bar.open`, so keymaps can
/// bind them: `kiln::init` calls it before it checks the app's keymap. An
/// app not set up through `kiln::init` calls it itself, before its keymap
/// is checked. It also installs the picker's commands (`picker.*` and
/// `text_input.*`), which keymaps are checked against through
/// [`catalog`].
pub fn register(cx: &mut App) {
    menu_bar::register(cx);
    picker::init(cx);
}

/// The shell's commands, to check keymap bindings against without an app:
/// `menu_bar.open`, and the picker's.
pub fn catalog() -> Box<dyn CommandCatalog> {
    Box::new(Catalog(vec![menu_bar::catalog(), picker::catalog()]))
}

/// Each catalog in turn, until one knows the command.
struct Catalog(Vec<Box<dyn CommandCatalog>>);

impl CommandCatalog for Catalog {
    fn check(&self, command: &CommandId, args: &Value) -> Result<(), CatalogError> {
        for catalog in &self.0 {
            match catalog.check(command, args) {
                Err(CatalogError::UnknownCommand) => {}
                result => return result,
            }
        }
        Err(CatalogError::UnknownCommand)
    }
}
