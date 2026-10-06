//! The native menu bar, from a [`MenuBar`] table: [`install`] sets GPUI's
//! menus, and choosing an item dispatches the same [`RunCommand`] a key
//! would, stamped with the focused element's context and
//! [`fe_command::Source::Menu`].
//!
//! Known limit: every item is enabled. AppKit asks
//! [`App::is_action_available`] about each item, which in gpui-pre 0.3.7
//! looks only at the action's type and global listeners, and overrides the
//! `disabled` flag given to `set_menus`. An item whose command doesn't apply
//! where focus is runs nothing, as its key wouldn't.

use fe_command::ContextStack;
use fe_keymap::{CommandItem, ItemKind, MenuBar, OsAction, SystemMenu};
use gpui::{Action, App, Global, KeyBinding, Menu, MenuItem, SystemMenuType};

use crate::{commands, keymap};

/// The key context of the menus' display-only key bindings, which no
/// element sets.
pub const KEY_CONTEXT: &str = "fastedit_menu";

/// What every menu item dispatches: the item, by its menu's name and its
/// ID, looked up when chosen.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = fastedit, no_json)]
pub struct MenuCommand {
    pub menu: String,
    pub item: String,
}

struct Installed(MenuBar);

impl Global for Installed {}

impl Installed {
    fn item(&self, command: &MenuCommand) -> Option<&CommandItem> {
        self.0.menu(&command.menu)?.item(&command.item)?.command()
    }
}

/// Sets `menus` as the app's menu bar, each item showing the single key
/// the keymap runs it with, if any. They're set again whenever
/// [`crate::set_user_keymap`] swaps the keymap. Call after [`crate::init`].
pub fn install(cx: &mut App, menus: MenuBar) {
    if !cx.has_global::<Installed>() {
        // Nothing in the element tree handles `MenuCommand`, so it always
        // gets here.
        cx.on_action(choose);
    }
    cx.set_global(Installed(menus));
    refresh(cx);
}

/// Rebuilds the installed menus and their key bindings from the current
/// keymap. Without installed menus, it does nothing.
pub(crate) fn refresh(cx: &mut App) {
    let Some(Installed(bar)) = cx.try_global::<Installed>() else {
        return;
    };
    let keymap = keymap::current(cx);
    let mut bindings = Vec::new();
    let mut menus = Vec::new();
    for menu in &bar.menus {
        let mut items = Vec::new();
        for item in &menu.items {
            match &item.kind {
                ItemKind::Command(command) => {
                    let action = MenuCommand {
                        menu: menu.name.clone(),
                        item: command.id.clone(),
                    };
                    if let Some(keys) = command.shortcut(&keymap) {
                        let keys = keys.to_string();
                        if gpui::Keystroke::parse(&keys).is_ok() {
                            // Display only: macOS reads a menu item's key
                            // equivalent from GPUI's keymap, so the shortcut
                            // must be bound there. No element sets
                            // `KEY_CONTEXT`, so GPUI never matches it; keys
                            // go through `fe-keymap` (see `crate::keymap`).
                            // This is fastedit's one GPUI key binding.
                            bindings.push(KeyBinding::new(
                                &keys,
                                action.clone(),
                                Some(KEY_CONTEXT),
                            ));
                        } else {
                            log::debug!("menu shortcut {keys} isn't a GPUI keystroke; not shown");
                        }
                    }
                    items.push(match command.os_action {
                        Some(os_action) => MenuItem::os_action(
                            command.title.clone(),
                            action,
                            gpui_os_action(os_action),
                        ),
                        None => MenuItem::action(command.title.clone(), action),
                    });
                }
                ItemKind::Separator => items.push(MenuItem::separator()),
                ItemKind::System(system) => {
                    let menu_type = match system.menu {
                        SystemMenu::Services => SystemMenuType::Services,
                    };
                    items.push(MenuItem::os_submenu(system.title.clone(), menu_type));
                }
                // Only for extending a bar.
                ItemKind::Remove(_) => {}
            }
        }
        menus.push(Menu::new(menu.name.clone()).items(items));
    }
    cx.clear_key_bindings();
    cx.bind_keys(bindings);
    cx.set_menus(menus);
}

fn gpui_os_action(action: OsAction) -> gpui::OsAction {
    match action {
        OsAction::Cut => gpui::OsAction::Cut,
        OsAction::Copy => gpui::OsAction::Copy,
        OsAction::Paste => gpui::OsAction::Paste,
        OsAction::SelectAll => gpui::OsAction::SelectAll,
        OsAction::Undo => gpui::OsAction::Undo,
        OsAction::Redo => gpui::OsAction::Redo,
    }
}

/// Runs the chosen item as a key would, in the active window's focused
/// context; with no window, through the app's commands, in no context.
fn choose(command: &MenuCommand, cx: &mut App) {
    let Some(item) = cx
        .try_global::<Installed>()
        .and_then(|installed| installed.item(command))
        .cloned()
    else {
        log::warn!("no menu item {} > {}", command.menu, command.item);
        return;
    };
    let Some(handle) = cx.active_window() else {
        commands::dispatch_app(&item.invocation(&ContextStack::new()), cx);
        return;
    };
    // GPUI has the window out while it dispatches this action to it, so
    // it's updated once that's over.
    cx.defer(move |cx| {
        let ran = handle.update(cx, |_, window, cx| {
            let stack = keymap::context_stack(&window.context_stack());
            keymap::dispatch(item.invocation(&stack), window, cx);
        });
        if ran.is_err() {
            // It closed meanwhile.
            commands::dispatch_app(&item.invocation(&ContextStack::new()), cx);
        }
    });
}
