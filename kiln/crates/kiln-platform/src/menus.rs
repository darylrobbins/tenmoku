// SPDX-License-Identifier: Apache-2.0

//! The native menu bar, from a [`MenuBar`] table: [`install`] sets GPUI's
//! menus, and choosing an item dispatches its command as a key would
//! ([`crate::keys`]), stamped with the focused element's context and
//! [`kiln_command::Source::Menu`].
//!
//! Known limit: every item is enabled. AppKit asks
//! [`App::is_action_available`] about each item, which in gpui-pre 0.3.7
//! looks only at the action's type and global listeners, and overrides the
//! `disabled` flag given to `set_menus`. An item whose command doesn't apply
//! where focus is runs nothing, as its key wouldn't.

use gpui::{Action, App, Global, KeyBinding, Menu, MenuItem, SystemMenuType, Window};
use kiln_command::ContextStack;
use kiln_core::{CommandItem, ItemKind, Key, MenuBar, OsAction, SystemMenu};

use crate::{commands, keys};

/// The key context of the menus' display-only key bindings, which no
/// element sets.
pub const KEY_CONTEXT: &str = "kiln_menu";

/// What every menu item dispatches: the item, by its menu's ID and its
/// own, looked up when chosen.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = kiln, no_json)]
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
/// [`keys::set_keymap`] swaps the keymap. Windows redraw, so a menu bar
/// drawn in one follows. Call after [`keys::init`].
pub fn install(cx: &mut App, menus: MenuBar) {
    if !cx.has_global::<Installed>() {
        // Nothing in the element tree handles `MenuCommand`, so it always
        // gets here.
        cx.on_action(choose);
    }
    cx.set_global(Installed(menus));
    refresh(cx);
    cx.refresh_windows();
}

/// The menus [`install`] set, if any: for a menu bar the host draws itself
/// where the platform has none.
pub fn installed(cx: &App) -> Option<&MenuBar> {
    cx.try_global::<Installed>().map(|installed| &installed.0)
}

/// Runs `menu`'s item `item` as choosing it in a native menu does, in
/// `window`'s focused context: for a menu bar drawn in the window. An item
/// that isn't installed does nothing.
pub fn run(menu: &str, item: &str, window: &mut Window, cx: &mut App) {
    let Some(item) = installed(cx)
        .and_then(|bar| bar.menu(menu)?.item(item)?.command())
        .cloned()
    else {
        log::warn!("no menu item {menu} > {item}");
        return;
    };
    let stack = keys::context_stack(&window.context_stack(), cx);
    keys::dispatch(item.invocation(&stack), window, cx);
}

/// Rebuilds the installed menus and their key bindings from the current
/// keymap. Without installed menus, it does nothing. [`keys::set_keymap`]
/// calls it.
pub fn refresh(cx: &mut App) {
    let Some(Installed(bar)) = cx.try_global::<Installed>() else {
        return;
    };
    let keymap = keys::current_keymap(cx);
    let mut bindings = Vec::new();
    let mut menus = Vec::new();
    let window_menu = windows_menu(bar);
    for menu in &bar.menus {
        let mut items = Vec::new();
        for item in &menu.items {
            match &item.kind {
                ItemKind::Command(command) => {
                    let action = MenuCommand {
                        menu: menu.id.clone(),
                        item: command.id.clone(),
                    };
                    // The newest single-keystroke key macOS can display; a
                    // newer one it can't is skipped, not the whole shortcut.
                    let candidates = kiln_core::shortcuts(
                        &keymap,
                        &command.command,
                        &command.args,
                        &command.shortcut_context,
                    )
                    .into_iter()
                    .filter(|keys| matches!(keys.keystrokes(), [key] if key.key != Key::Click));
                    for keys in candidates {
                        let keys = keys.to_string();
                        let keystroke = gpui::Keystroke::parse(&keys);
                        if keystroke.as_ref().is_ok_and(native_key_equivalent) {
                            // Display only: macOS reads a menu item's key
                            // equivalent from GPUI's keymap, so the shortcut
                            // must be bound there. No element sets
                            // `KEY_CONTEXT`, so GPUI never matches it; keys
                            // go through kiln's keymap (see `crate::keys`).
                            bindings.push(KeyBinding::new(
                                &keys,
                                action.clone(),
                                Some(KEY_CONTEXT),
                            ));
                            break;
                        } else if keystroke.is_ok() {
                            log::debug!(
                                "menu shortcut {keys} has no native key equivalent; not shown"
                            );
                        } else {
                            log::debug!("menu shortcut {keys} isn't a GPUI keystroke; not shown");
                        }
                    }
                    let title = command.label().resolve();
                    items.push(match command.os_action {
                        Some(os_action) => {
                            MenuItem::os_action(title, action, gpui_os_action(os_action))
                        }
                        None => MenuItem::action(title, action),
                    });
                }
                ItemKind::Separator => items.push(MenuItem::separator()),
                ItemKind::System(system) => {
                    let menu_type = match system.menu {
                        SystemMenu::Services => SystemMenuType::Services,
                    };
                    items.push(MenuItem::os_submenu(system.label().resolve(), menu_type));
                }
                // Only for extending a bar.
                ItemKind::Remove(_) => {}
            }
        }
        menus.push(Menu::new(menu.label().resolve()).items(items));
    }
    // GPUI's keymap is shared: a widget's own bindings, such as a text
    // field's, stay, and only the previous menus' are replaced.
    let others: Vec<KeyBinding> = cx
        .key_bindings()
        .borrow()
        .bindings()
        .filter(|binding| !binding.action().as_any().is::<MenuCommand>())
        .cloned()
        .collect();
    cx.clear_key_bindings();
    cx.bind_keys(others.into_iter().chain(bindings));
    cx.set_menus(menus);
    #[cfg(target_os = "macos")]
    if let Some(ix) = window_menu {
        macos::set_windows_menu(ix);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window_menu;
}

/// The ID of the menu macOS keeps the window list in: kiln's bundled
/// Window menu's, its en-US name, whatever its title in the active locale.
pub const WINDOW_MENU_ID: &str = "Window";

/// The index of the menu GPUI's macOS backend doesn't make the app's
/// Windows menu itself: the one whose ID is [`WINDOW_MENU_ID`], when its
/// title in the active locale isn't `Window`, which is all gpui-pre-macos
/// looks for. [`refresh`] registers it with AppKit after setting the
/// menus, at startup and after every locale switch, so the role follows
/// the menu, not its title.
pub fn windows_menu(bar: &MenuBar) -> Option<usize> {
    let ix = bar
        .menus
        .iter()
        .position(|menu| menu.id == WINDOW_MENU_ID)?;
    (bar.menus[ix].label().resolve() != WINDOW_MENU_ID).then_some(ix)
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::{class, msg_send, runtime::AnyObject};

    /// Makes the submenu of the main menu's item `ix` AppKit's Windows
    /// menu, as gpui-pre-macos does for one titled `Window`. Only on the
    /// main thread, and only when there's a main menu with such an item:
    /// tests set their menus on a headless platform, with none.
    pub(super) fn set_windows_menu(ix: usize) {
        // SAFETY: messages to AppKit's own classes and the objects they
        // return, each checked for nil before it's sent another, on the
        // main thread.
        unsafe {
            let main_thread: bool = msg_send![class!(NSThread), isMainThread];
            if !main_thread {
                return;
            }
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let Some(app) = app.as_ref() else {
                return;
            };
            let main_menu: *mut AnyObject = msg_send![app, mainMenu];
            let Some(main_menu) = main_menu.as_ref() else {
                return;
            };
            let count: isize = msg_send![main_menu, numberOfItems];
            let Ok(ix) = isize::try_from(ix) else {
                return;
            };
            if ix >= count {
                return;
            }
            let item: *mut AnyObject = msg_send![main_menu, itemAtIndex: ix];
            let Some(item) = item.as_ref() else {
                return;
            };
            let submenu: *mut AnyObject = msg_send![item, submenu];
            if let Some(submenu) = submenu.as_ref() {
                let () = msg_send![app, setWindowsMenu: submenu];
            }
        }
    }
}

/// The keys gpui-pre-macos 0.3.7's `key_to_native` converts for a key
/// equivalent. Any other key reaches AppKit as its name, so only a single
/// character is shown as itself.
const NATIVE_KEYS: &[&str] = &[
    "space",
    "backspace",
    "escape",
    "up",
    "down",
    "left",
    "right",
    "pageup",
    "pagedown",
    "home",
    "end",
    "delete",
    "insert",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "f13",
    "f14",
    "f15",
    "f16",
    "f17",
    "f18",
    "f19",
    "f20",
    "f21",
    "f22",
    "f23",
    "f24",
    "f25",
    "f26",
    "f27",
    "f28",
    "f29",
    "f30",
    "f31",
    "f32",
    "f33",
    "f34",
    "f35",
];

/// Whether `keystroke` reaches AppKit as itself, so a menu can show it.
/// GPUI's macOS menu builder drops the fn modifier and passes an
/// unconverted key, such as `enter` or `tab`, by name; AppKit could then
/// run the item on keys the keymap doesn't bind to it, such as `cmd-s` for
/// `cmd-fn-s`.
pub fn native_key_equivalent(keystroke: &gpui::Keystroke) -> bool {
    let mut chars = keystroke.key.chars();
    let one_char = chars.next().is_some() && chars.next().is_none();
    !keystroke.modifiers.function && (one_char || NATIVE_KEYS.contains(&keystroke.key.as_str()))
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

/// What choosing a native menu item does, as [`install`]'s handler for
/// [`MenuCommand`]: runs the item as a key would, in the active window's
/// focused context; with no window, through the app's commands, in no
/// context. If that window closes before it runs, only an `app` command
/// runs, windowless.
pub fn choose(command: &MenuCommand, cx: &mut App) {
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
    let command = command.clone();
    cx.defer(move |cx| {
        let ran = handle.update(cx, |_, window, cx| {
            let stack = keys::context_stack(&window.context_stack(), cx);
            keys::dispatch(item.invocation(&stack), window, cx);
        });
        if ran.is_err() {
            // It closed meanwhile. Only an app-wide command still applies:
            // a window one would act on whichever window is active now.
            let invocation = item.invocation(&ContextStack::new());
            if invocation.command.namespace() == "app" {
                commands::dispatch_app(&invocation, cx);
            } else {
                log::debug!(
                    "menu item {} > {}: its window closed",
                    command.menu,
                    command.item
                );
            }
        }
    });
}

/// Which menu [`refresh`] registers as AppKit's Windows menu: the one whose
/// ID is `Window` once its title is translated, in a fake locale and back.
/// The process catalogue is global: this test switches it, and is its own
/// process under nextest.
#[cfg(test)]
mod tests {
    use std::path::Path;

    use kiln_core::MenuBar;
    use kiln_l10n::{LanguageIdentifier, Registered, ResourceFile, Resources, set_locales};

    use super::windows_menu;

    static FAKE_LOCALE: Resources = Resources {
        crate_name: "kiln-platform-window-menu-fake-locale",
        manifest_dir: env!("CARGO_MANIFEST_DIR"),
        declared_in: file!(),
        files: &[ResourceFile {
            locale: "qps-ploc",
            path: "qps-ploc/menus.ftl",
            source: "kiln-core-menu-window = ⟦Wíndów⟧\n",
        }],
        dynamic_prefixes: &[],
    };

    kiln_l10n::__private::inventory::submit! { Registered(&FAKE_LOCALE) }

    fn bar(source: &str) -> MenuBar {
        let (bar, diagnostics) = MenuBar::parse(Path::new("menus.toml"), source, None);
        assert_eq!(diagnostics, []);
        bar.unwrap()
    }

    #[test]
    fn the_window_menu_keeps_its_role_when_translated() {
        let keyed =
            bar("[[menus]]\nname = \"Pad\"\n\n[[menus]]\ntitle_key = \"kiln-core-menu-window\"\n");
        let literal = bar("[[menus]]\nname = \"Pad\"\n\n[[menus]]\nname = \"Window\"\n");
        assert_eq!(keyed.menus[1].id, "Window");
        // In en-US, GPUI's backend finds it by its title itself.
        assert_eq!(windows_menu(&keyed), None);
        assert_eq!(windows_menu(&literal), None);

        let fake: Vec<LanguageIdentifier> = vec!["qps-ploc".parse().unwrap()];
        set_locales(&fake);
        let translated = (windows_menu(&keyed), windows_menu(&literal));
        set_locales(&[]);
        let back = windows_menu(&keyed);

        assert_eq!(translated, (Some(1), None));
        assert_eq!(back, None);
    }
}
