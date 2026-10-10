// SPDX-License-Identifier: LicenseRef-Proprietary

//! The native menu bar in headless GPUI: `get_menus()` and `key_bindings()`
//! show what would reach AppKit, and dispatching a `MenuCommand` is what
//! choosing an item does.

use std::{cell::RefCell, path::Path, rc::Rc};

use gpui::{
    Action as _, AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, OwnedMenuItem, ParentElement as _, Render, Styled as _, TestAppContext,
    VisualTestContext, Window, WindowHandle, div,
};
use kiln_command::{ContextFrame, ContextStack, Invocation, Source};
use kiln_core::{Keymap, KeymapLayer, MenuBar, Os};
use kiln_platform::{
    commands::{self, RunCommand, observe_invocations},
    keys,
    menus::{self, KEY_CONTEXT, MenuCommand, choose, native_key_equivalent},
};

mod pad {
    kiln_platform::commands!(
        pad,
        [
            /// Clear
            Clear,
        ]
    );
}

const MENUS: &str = r#"
[[menus]]
name = "Pad"

[[menus.items]]
title = "Services"
system = "services"

[[menus.items]]
separator = true

[[menus.items]]
title = "Quit"
command = "app.quit"

[[menus]]
name = "Edit"

[[menus.items]]
title = "Select All"
command = "pad.select_all"
os_action = "select_all"

[[menus.items]]
title = "Fold All"
command = "pad.fold_all"

[[menus.items]]
title = "Clear"
command = "pad.clear"

[[menus.items]]
title = "Save"
command = "pad.save"

[[menus]]
name = "View"

[[menus.items]]
title = "Toggle Full Screen"
command = "window.toggle_fullscreen"

[[menus]]
name = "Window"

[[menus.items]]
title = "Close Window"
command = "window.close"

[[menus.items]]
title = "Minimize"
command = "window.minimize"

[[menus.items]]
title = "Zoom"
command = "window.zoom"
"#;

const KEYS: &str = r#"
[[bindings]]
keys = "secondary-a"
command = "pad.select_all"

[[bindings]]
keys = "secondary-k secondary-0"
command = "pad.fold_all"

[[bindings]]
keys = "secondary-s"
command = "pad.save"
"#;

fn bar() -> MenuBar {
    let (bar, diagnostics) = MenuBar::parse(Path::new("menus.toml"), MENUS, None);
    assert_eq!(diagnostics, []);
    bar.unwrap()
}

/// The keymap of `KEYS` under a user layer of `user`.
fn keymap(user: &str) -> Keymap {
    let layer = |source: &str| {
        let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), source, None);
        assert_eq!(diagnostics, []);
        layer.unwrap()
    };
    Keymap::from_layers([layer(KEYS), layer(user)])
}

/// `keys` with this platform's `secondary`, in GPUI's spelling.
fn gpui_keys(keys: &str) -> String {
    let secondary = if Os::current() == Os::MacOs {
        "cmd"
    } else {
        "ctrl"
    };
    gpui::Keystroke::parse(&keys.replace("secondary", secondary))
        .unwrap()
        .unparse()
}

/// The app's commands and keys, then the menus installed.
fn install(cx: &mut TestAppContext) {
    cx.update(|cx| {
        commands::init(cx, "Pad");
        pad::register(cx);
        keys::init(cx, keymap(""));
        menus::install(cx, bar());
    });
}

fn command(menu: &str, item: &str) -> MenuCommand {
    MenuCommand {
        menu: menu.into(),
        item: item.into(),
    }
}

/// Each GPUI key binding for `action`, as `(keys, context)`.
fn bindings_for(cx: &mut TestAppContext, action: &MenuCommand) -> Vec<(String, String)> {
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        keymap
            .bindings_for_action(action)
            .map(|binding| {
                let keys: Vec<_> = binding
                    .keystrokes()
                    .iter()
                    .map(|keystroke| keystroke.inner().unparse())
                    .collect();
                let context = binding.predicate().map(|p| p.to_string());
                (keys.join(" "), context.unwrap_or_default())
            })
            .collect()
    })
}

/// How many GPUI key bindings there are.
fn binding_count(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| cx.key_bindings().borrow().bindings().count())
}

/// Every invocation observers hear of from now on.
fn record(cx: &mut TestAppContext) -> Rc<RefCell<Vec<Invocation>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|cx| {
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        })
    });
    seen
}

#[gpui::test]
fn install_sets_menus_and_display_bindings(cx: &mut TestAppContext, bare: &mut TestAppContext) {
    assert_eq!(MenuCommand::name_for_type(), "kiln::MenuCommand");
    install(cx);
    let menus = cx.update(|cx| cx.get_menus()).unwrap();
    let names: Vec<_> = menus.iter().map(|menu| menu.name.to_string()).collect();
    assert_eq!(names, ["Pad", "Edit", "View", "Window"]);
    let edit = &menus[1];
    let select_all = edit.items.iter().find_map(|item| match item {
        OwnedMenuItem::Action {
            name,
            action,
            os_action,
            ..
        } if name == "Select All" => Some((
            action.as_any().downcast_ref::<MenuCommand>().cloned(),
            *os_action == Some(gpui::OsAction::SelectAll),
        )),
        _ => None,
    });
    assert_eq!(
        select_all,
        Some((Some(command("Edit", "pad.select_all")), true)),
        "with its OS action"
    );
    assert!(matches!(
        menus[0].items.first(),
        Some(OwnedMenuItem::SystemMenu(services)) if services.name == "Services"
    ));
    assert!(matches!(menus[0].items[1], OwnedMenuItem::Separator));

    assert_eq!(
        bindings_for(cx, &command("Edit", "pad.save")),
        [(gpui_keys("secondary-s"), KEY_CONTEXT.to_owned())]
    );
    // A chord can't show.
    assert_eq!(bindings_for(cx, &command("Edit", "pad.fold_all")), []);

    let all = cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        keymap
            .bindings()
            .map(|binding| {
                let ours = binding.action().as_any().is::<MenuCommand>();
                let context = binding.predicate().map(|p| p.to_string());
                (ours, context)
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(all.len(), 2);
    for (ours, context) in all {
        assert!(ours);
        assert_eq!(context.as_deref(), Some(KEY_CONTEXT));
    }
    assert_eq!(cx.update(|cx| menus::installed(cx).cloned()), Some(bar()));

    // Without menus, GPUI's keymap stays empty.
    bare.update(|cx| keys::init(cx, keymap("")));
    assert_eq!(binding_count(bare), 0);
    assert_eq!(bare.update(|cx| menus::installed(cx).cloned()), None);
}

#[gpui::test]
fn set_keymap_refreshes_menu_bindings(cx: &mut TestAppContext, bare: &mut TestAppContext) {
    install(cx);
    let save = command("Edit", "pad.save");
    // A widget's own GPUI binding, which the menus' leave be.
    cx.update(|cx| {
        cx.bind_keys([gpui::KeyBinding::new(
            "backspace",
            pad::Clear,
            Some("text_field"),
        )])
    });
    let user = r#"
        [[bindings]]
        keys = "alt-secondary-s"
        command = "pad.save"

        [[bindings]]
        keys = "secondary-s"
        command = ""
    "#;
    cx.update(|cx| keys::set_keymap(cx, keymap(user)));
    assert_eq!(
        bindings_for(cx, &save),
        [(gpui_keys("alt-secondary-s"), KEY_CONTEXT.to_owned())]
    );
    let secondary_s = gpui::Keystroke::parse(&gpui_keys("secondary-s")).unwrap();
    let with_secondary_s = cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        keymap
            .bindings()
            .filter(|binding| binding.keystrokes()[0].inner() == &secondary_s)
            .count()
    });
    assert_eq!(with_secondary_s, 0);
    assert_eq!(cx.update(|cx| cx.get_menus()).unwrap().len(), 4);
    let widget = cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        keymap.bindings_for_action(&pad::Clear).count()
    });
    assert_eq!(widget, 1, "the widget's binding is kept");
    assert_eq!(binding_count(cx), 3);

    // A host without menus is left alone.
    bare.update(|cx| {
        keys::init(cx, keymap(""));
        keys::set_keymap(cx, keymap(user));
    });
    assert_eq!(bare.update(|cx| cx.get_menus()).unwrap().len(), 0);
    assert_eq!(binding_count(bare), 0);
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
        let keystroke = gpui::Keystroke::parse(keys).unwrap();
        assert!(native_key_equivalent(&keystroke), "{keys}");
    }
    for keys in ["cmd-fn-s", "fn-f5", "cmd-enter", "ctrl-tab", "shift-enter"] {
        let keystroke = gpui::Keystroke::parse(keys).unwrap();
        assert!(!native_key_equivalent(&keystroke), "{keys}");
    }
}

#[gpui::test]
fn shortcuts_without_a_native_key_equivalent_are_not_shown(cx: &mut TestAppContext) {
    install(cx);
    let save = command("Edit", "pad.save");
    for keys in ["secondary-fn-s", "secondary-enter", "ctrl-tab"] {
        let user = format!(
            r#"
            [[bindings]]
            keys = "{keys}"
            command = "pad.save"

            [[bindings]]
            keys = "secondary-s"
            command = ""
            "#
        );
        cx.update(|cx| keys::set_keymap(cx, keymap(&user)));
        assert_eq!(bindings_for(cx, &save), [], "{keys}");
    }
    // The menus are still set, Save without a shortcut.
    assert_eq!(cx.update(|cx| cx.get_menus()).unwrap().len(), 4);
}

/// A focusable element in context `pad` that records the commands it runs.
struct Pad {
    focus: FocusHandle,
    ran: Rc<RefCell<Vec<Invocation>>>,
}

impl Render for Pad {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .key_context("pad")
            .size_full()
            .on_action(cx.listener(|this, run: &RunCommand, _, cx| {
                if run.0.command.namespace() == "pad" {
                    this.ran.borrow_mut().push(run.0.clone());
                } else {
                    commands::propagate_with_context(run.0.clone(), cx);
                }
            }))
            .on_action(cx.listener(|this, _: &pad::Clear, _, cx| {
                let invocation = kiln_platform::command_actions::current_invocation(cx);
                this.ran.borrow_mut().push(invocation.unwrap());
            }))
    }
}

/// A pad inside a `shell` key context.
struct Shell {
    pad: Entity<Pad>,
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("shell")
            .size_full()
            .child(self.pad.clone())
    }
}

/// An active window with a focused pad in a shell, and what the pad runs.
fn shell(cx: &mut TestAppContext) -> (Rc<RefCell<Vec<Invocation>>>, &mut VisualTestContext) {
    let ran = Rc::new(RefCell::new(Vec::new()));
    let (shell, cx) = cx.add_window_view({
        let ran = ran.clone();
        move |_, cx| Shell {
            pad: cx.new(|cx| Pad {
                focus: cx.focus_handle(),
                ran,
            }),
        }
    });
    let focus = shell.update(cx, |shell, cx| shell.pad.read(cx).focus.clone());
    // Menu items run in the active window, as AppKit's do.
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    (ran, cx)
}

fn in_shell(command: &str) -> Invocation {
    let shell_pad = ContextStack::new()
        .with(ContextFrame::new("shell"))
        .with(ContextFrame::new("pad"));
    Invocation::of(command)
        .unwrap()
        .source(Source::Menu)
        .context(shell_pad)
}

#[gpui::test]
fn choosing_an_item_runs_in_the_focused_context(
    cx: &mut TestAppContext,
    windowless: &mut TestAppContext,
) {
    install(cx);
    let (ran, cx) = shell(cx);

    // Chosen natively, and from a menu bar drawn in the window.
    cx.dispatch_action(command("Edit", "pad.select_all"));
    assert_eq!(ran.take(), [in_shell("pad.select_all")]);
    cx.update(|window, cx| menus::run("Edit", "pad.save", window, cx));
    assert_eq!(ran.take(), [in_shell("pad.save")]);
    // A typed command runs as its action.
    cx.dispatch_action(command("Edit", "pad.clear"));
    assert_eq!(ran.take(), [in_shell("pad.clear")]);
    // An item that isn't installed does nothing.
    cx.update(|window, cx| menus::run("Edit", "pad.missing", window, cx));
    assert_eq!(ran.take(), []);

    // An app command goes on to the app's registry.
    let fullscreen = command("View", "window.toggle_fullscreen");
    assert!(!cx.update(|window, _| window.is_fullscreen()));
    cx.dispatch_action(fullscreen.clone());
    assert!(cx.update(|window, _| window.is_fullscreen()));
    cx.update(|window, cx| menus::run("View", "window.toggle_fullscreen", window, cx));
    cx.run_until_parked();
    assert!(!cx.update(|window, _| window.is_fullscreen()));

    // With no window, through the app's commands, in no context.
    install(windowless);
    let seen = record(windowless);
    windowless.update(|cx| cx.dispatch_action(&fullscreen));
    let toggle = Invocation::of("window.toggle_fullscreen")
        .unwrap()
        .source(Source::Menu);
    assert_eq!(*seen.borrow(), [toggle]);
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
    let seen = record(cx);
    let b = open(cx);
    for item in [
        command("Window", "window.close"),
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
    choose_as_a_closes(cx, &command("Pad", "app.quit"), a, b);
    let quit = Invocation::of("app.quit").unwrap().source(Source::Menu);
    assert_eq!(*seen.borrow(), [quit]);
}
