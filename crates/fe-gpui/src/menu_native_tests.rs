//! What reaches AppKit and when a chosen item still runs: shortcuts its
//! menu builder can't show as themselves stay out of GPUI's keymap, and an
//! item whose window closed before it ran doesn't act on another window.

use std::{cell::RefCell, path::Path, rc::Rc};

use fe_command::{Invocation, Source};
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, Keymap, KeymapLayer};
use gpui::{Context, IntoElement, Render, TestAppContext, Window, WindowHandle, div};

use super::{MenuCommand, choose, native};
use crate::{commands::observe_invocations, init, menu, set_user_keymap};

fn install(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init(cx);
        let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
        assert_eq!(diagnostics, []);
        menu::install(cx, bar.unwrap());
    });
}

fn command(menu: &str, item: &str) -> MenuCommand {
    MenuCommand {
        menu: menu.into(),
        item: item.into(),
    }
}

/// The default keymap under a user layer of `user`.
fn keymap(user: &str) -> Keymap {
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    let (user, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), user, None);
    assert_eq!(diagnostics, []);
    Keymap::from_layers([default.unwrap(), user.unwrap()])
}

/// GPUI's keys for each of `action`'s display bindings.
fn keys_for(cx: &mut TestAppContext, action: &MenuCommand) -> Vec<String> {
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        keymap
            .bindings_for_action(action)
            .map(|binding| binding.keystrokes()[0].inner().unparse())
            .collect()
    })
}

#[test]
fn only_keys_appkit_gets_as_themselves_are_native() {
    for keys in [
        "cmd-s",
        "alt-cmd-s",
        "ctrl-shift-=",
        "cmd-space",
        "cmd-left",
        "f5",
        "cmd-f20",
    ] {
        assert!(native(&gpui::Keystroke::parse(keys).unwrap()), "{keys}");
    }
    for keys in ["cmd-fn-s", "fn-f5", "cmd-enter", "ctrl-tab", "shift-enter"] {
        assert!(!native(&gpui::Keystroke::parse(keys).unwrap()), "{keys}");
    }
}

#[gpui::test]
fn shortcuts_without_a_native_key_equivalent_are_not_shown(cx: &mut TestAppContext) {
    install(cx);
    let save = command("File", "editor.save");
    for keys in ["cmd-fn-s", "cmd-enter", "ctrl-tab"] {
        // AppKit would get `cmd-s`, `cmd-` plus "enter", and so on, though
        // `fe-keymap` binds none of them to Save.
        let user = format!(
            r#"
            [[bindings]]
            keys = "{keys}"
            command = "editor.save"
            context = "editor"

            [[bindings]]
            keys = "cmd-s"
            command = ""
            context = "editor"
            "#
        );
        cx.update(|cx| set_user_keymap(cx, keymap(&user)));
        assert_eq!(keys_for(cx, &save), Vec::<String>::new(), "{keys}");
    }
    let cmd_s = gpui::Keystroke::parse("cmd-s").unwrap();
    let with_cmd_s = cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        keymap
            .bindings()
            .filter(|binding| binding.keystrokes()[0].inner() == &cmd_s)
            .count()
    });
    assert_eq!(with_cmd_s, 0);
    // The menus are still set, Save without a shortcut.
    assert_eq!(cx.update(|cx| cx.get_menus()).unwrap().len(), 6);
}

struct Blank;

impl Render for Blank {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Opens an active window.
fn open(cx: &mut TestAppContext) -> WindowHandle<Blank> {
    let window = cx.add_window(|_, _| Blank);
    window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    window
}

/// Chooses `item` with `a` active; `a` closes and `b` becomes active
/// before it runs.
fn choose_as_a_closes(
    cx: &mut TestAppContext,
    item: &MenuCommand,
    a: WindowHandle<Blank>,
    b: WindowHandle<Blank>,
) {
    assert_eq!(cx.update(|cx| cx.active_window()), Some(a.into()));
    cx.update(|cx| {
        choose(item, cx);
        a.update(cx, |_, window, _| window.remove_window()).unwrap();
        b.update(cx, |_, window, _| window.activate_window())
            .unwrap();
    });
    cx.run_until_parked();
}

#[gpui::test]
fn a_window_item_is_dropped_once_its_window_closes(cx: &mut TestAppContext) {
    install(cx);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|cx| {
        observe_invocations(cx, move |invocation: &Invocation| {
            sink.borrow_mut().push(invocation.clone())
        })
    });
    let b = open(cx);
    for item in [
        command("File", "window.close"),
        command("View", "window.toggle_fullscreen"),
        command("Window", "window.minimize"),
        command("Window", "window.zoom"),
    ] {
        let a = open(cx);
        choose_as_a_closes(cx, &item, a, b);
        assert_eq!(*seen.borrow(), [], "{}", item.item);
        let fullscreen = b.update(cx, |_, window, _| window.is_fullscreen());
        assert_eq!(fullscreen.ok(), Some(false), "{}", item.item);
    }

    // An app item still runs, windowless.
    let a = open(cx);
    choose_as_a_closes(cx, &command("Tenmoku", "app.quit"), a, b);
    let quit = Invocation::of("app.quit").unwrap().source(Source::Menu);
    assert_eq!(*seen.borrow(), [quit]);
}
