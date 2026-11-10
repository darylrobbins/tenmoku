// SPDX-License-Identifier: Apache-2.0

//! Kiln's app shell: the root view that draws the window's chrome
//! ([`Root`]), and the chrome's pieces for a host that lays out its own
//! root: the unified title bar ([`TitleBar`]), the in-window menu bar
//! ([`MenuBarView`]) and the frame drawn under client-side decorations
//! ([`window_frame`]). Each reads what the platform does natively from
//! [`Capabilities`], or GPUI's window decorations, and its colours from
//! the active theme.
//!
//! What a pane shows in a tab is a [`PaneItem`], held through an
//! [`ItemHandle`] ([`item`]). A pane is any [`PaneView`] over a
//! [`PaneState`], such as kiln's [`ItemPane`] ([`pane`]), and panes split
//! into a [`PaneGroup`] ([`pane_group`]), between which tabs move. The active item gives the pane's breadcrumbs
//! ([`crumbs`]) and the status bar's items ([`StatusItems::items_for`]).
//! Beside the panes, a [`Dock`] shows one of its registered panels
//! ([`DockItem`]) at a time ([`dock`]); over them, the [`ModalLayer`]
//! shows one modal view, such as a [`Picker`] ([`modal`]). A panel can
//! stack several views under collapsible headers in a [`ViewStack`]
//! ([`view_stack`]); a [`ViewContainer`] is such a panel, its views
//! registered by id and their state kept in saved layouts
//! ([`view_container`]).
//!
//! [`Shell`] is a window's shell around the host's centre view: the chrome,
//! the docks, the status bar and the modal layer, under the app's root key
//! context. Whatever closes items (a tab, a pane, a window, quitting) asks
//! them first ([`close`]). A host's root folder's workspace trust is [`trust`]'s.
//! [`Workspace`] is a shell whose centre is a group of panes, kept in step
//! for the host ([`workspace`]).
//!
//! The shell's leaf modules, which a host can use before it adopts the
//! rest: the status bar's items ([`status`]), a picker palette over a
//! delegate ([`picker`]), and a background writer for saved state
//! ([`persist`]).

pub mod close;
pub mod command_palette;
pub mod crumbs;
pub mod dock;
pub mod item;
pub mod l10n;
pub mod layout;
pub mod menu_bar;
pub mod modal;
pub mod pane;
pub mod pane_group;
pub mod persist;
pub mod picker;
mod root;
mod shell;
pub mod status;
mod title_bar;
pub mod trust;
pub mod view_container;
pub mod view_stack;
pub mod window_frame;
pub mod workspace;

use gpui::App;
use kiln_command::{CommandId, Value};
use kiln_core::{CatalogError, CommandCatalog};
use kiln_platform::Capabilities;

pub use close::{CloseKind, CloseRequest, Closing};
pub use dock::{Dock, DockItem, DockPosition, DockState, DockView, PanelId, RailAction};
// The frame-request watchdog, for a host whose windows aren't `Root`s.
pub use item::{CloseAnswer, ItemCrumb, ItemHandle, ItemId, PaneItem};
pub use kiln_platform::frame_watch::watch_frame_requests;
pub use menu_bar::MenuBarView;
pub use modal::{ModalEvent, ModalId, ModalLayer, ModalView};
pub use pane::{ItemPane, PaneEvent, PaneState, PaneView};
pub use pane_group::{Axis, Destination, Direction, Member, Members, PaneGroup};
pub use picker::{Picker, PickerDelegate};
pub use root::Root;
pub use shell::Shell;
pub use status::{StatusItem, StatusItems, StatusSide};
pub use title_bar::{TitleBar, TitleBarItems, TitleBarSlot};
pub use view_container::{ViewContainer, ViewSpec};
pub use view_stack::{StackedView, ViewId, ViewStack};
pub use workspace::{Workspace, WorkspaceEvent};

// For status items and tab icons, to crates that use kiln-shell without
// kiln-ui.
pub use kiln_ui::{Icon, IconRef, StatusFill, StatusTone, file_icon, file_icon_ref};

/// Sets up what the shell draws for the platform: where it has no native
/// alerts, `window.prompt` shows kiln-ui's in-window prompt, themed and
/// wrapped, instead of GPUI's plain fallback. `app.quit` and
/// `window.close` ask the windows' items first ([`close`]).
pub fn init(cx: &mut App) {
    close::init(cx);
    if !Capabilities::of(cx).native_prompts {
        cx.set_prompt_builder(kiln_ui::prompt_renderer);
    }
}

/// Records the shell's commands, such as `menu_bar.open` and
/// `command_palette.toggle`, so keymaps can bind them: `kiln::init` calls
/// it before it checks the app's keymap. An app not set up through
/// `kiln::init` calls it itself, before its keymap is checked. It also installs the picker's commands (`picker.*` and
/// `text_input.*`), which keymaps are checked against through
/// [`catalog`].
pub fn register(cx: &mut App) {
    menu_bar::register(cx);
    command_palette::register(cx);
    picker::init(cx);
}

/// The shell's commands, to check keymap bindings against without an app:
/// `menu_bar.open`, `command_palette.toggle`, and the picker's.
pub fn catalog() -> Box<dyn CommandCatalog> {
    Box::new(Catalog(vec![
        menu_bar::catalog(),
        command_palette::catalog(),
        picker::catalog(),
    ]))
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
