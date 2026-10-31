//! The native menu bar in headless GPUI: `get_menus()` and `key_bindings()`
//! show what would reach AppKit, and dispatching a `MenuCommand` is what
//! choosing an item does.

use std::{cell::RefCell, path::Path, rc::Rc};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation, Source};
use fe_editor::EditorModel;
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, Keymap, KeymapLayer, MenuBar};
use gpui::{
    AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    OwnedMenuItem, ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext,
    Window, div,
};

use crate::{
    EditorTheme, EditorView, commands::observe_invocations, init, menu, menu::MenuCommand,
    set_user_keymap,
};

fn default_bar() -> MenuBar {
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    bar.unwrap()
}

/// `init`, then the default menus installed.
fn install(cx: &mut TestAppContext) {
    cx.update(|cx| {
        init(cx);
        menu::install(cx, default_bar());
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

/// GPUI's spelling of `keys`.
fn gpui_keys(keys: &str) -> String {
    gpui::Keystroke::parse(keys).unwrap().unparse()
}

/// Every invocation run from now on.
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
    install(cx);
    let menus = cx.update(|cx| cx.get_menus()).unwrap();
    let names: Vec<_> = menus.iter().map(|menu| menu.name.to_string()).collect();
    assert_eq!(names, ["Tenmoku", "File", "Edit", "View", "Go", "Window"]);
    let view = &menus[3];
    let search = view.items.iter().find_map(|item| match item {
        OwnedMenuItem::Action { name, action, .. } if name == "Search" => {
            action.as_any().downcast_ref::<MenuCommand>().cloned()
        }
        _ => None,
    });
    assert_eq!(search, Some(command("View", "search.show")));
    assert!(matches!(
        menus[0].items.iter().find(|item| matches!(item, OwnedMenuItem::SystemMenu(_))),
        Some(OwnedMenuItem::SystemMenu(services)) if services.name == "Services"
    ));

    assert_eq!(
        bindings_for(cx, &command("View", "search.show")),
        [(gpui_keys("cmd-shift-f"), menu::KEY_CONTEXT.to_owned())]
    );
    // A chord can't show.
    assert_eq!(bindings_for(cx, &command("File", "pane.close")), []);
    assert_eq!(bindings_for(cx, &command("View", "editor.fold_all")), []);

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
    assert!(!all.is_empty());
    for (ours, context) in all {
        assert!(ours);
        assert_eq!(context.as_deref(), Some(menu::KEY_CONTEXT));
    }

    // Without menus, GPUI's keymap stays empty.
    bare.update(init);
    let count = bare.update(|cx| cx.key_bindings().borrow().bindings().count());
    assert_eq!(count, 0);
}

/// A focused editor showing `text`, with the caret at its end.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_cursor(text.len());
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    (view, cx)
}

#[gpui::test]
fn keys_still_dispatch_once(cx: &mut TestAppContext) {
    install(cx);
    let seen = record(cx);
    let (view, cx) = editor(cx, "abc");
    cx.simulate_keystrokes("cmd-a");
    let seen = std::mem::take(&mut *seen.borrow_mut());
    let select_all = Invocation::of("editor.select_all")
        .unwrap()
        .source(Source::Keyboard)
        .context(fe_editor::commands::context());
    assert_eq!(seen, [select_all]);
    let selection = view.update(cx, |view, _| view.model().selections()[0].range.clone());
    assert_eq!(selection, 0..3);
}

#[gpui::test]
fn keymap_reload_refreshes_menus(cx: &mut TestAppContext, bare: &mut TestAppContext) {
    install(cx);
    let save = command("File", "editor.save");
    assert_eq!(
        bindings_for(cx, &save),
        [(gpui_keys("cmd-s"), menu::KEY_CONTEXT.to_owned())]
    );
    let user = r#"
        [[bindings]]
        keys = "alt-cmd-s"
        command = "editor.save"
        context = "editor"

        [[bindings]]
        keys = "cmd-s"
        command = ""
        context = "editor"
    "#;
    let keymap = |user: &str| {
        let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
        let (user, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), user, None);
        assert_eq!(diagnostics, []);
        Keymap::from_layers([default.unwrap(), user.unwrap()])
    };
    cx.update(|cx| set_user_keymap(cx, keymap(user)));
    assert_eq!(
        bindings_for(cx, &save),
        [(gpui_keys("alt-cmd-s"), menu::KEY_CONTEXT.to_owned())]
    );
    let cmd_s = gpui::Keystroke::parse("cmd-s").unwrap();
    let with_cmd_s = cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        keymap
            .bindings()
            .filter(|binding| {
                binding
                    .keystrokes()
                    .iter()
                    .any(|keystroke| keystroke.inner() == &cmd_s)
            })
            .count()
    });
    assert_eq!(with_cmd_s, 0);
    assert_eq!(cx.update(|cx| cx.get_menus()).unwrap().len(), 6);

    // An embedding host without menus is left alone.
    bare.update(|cx| {
        init(cx);
        set_user_keymap(cx, keymap(user));
    });
    assert_eq!(bare.update(|cx| cx.get_menus()).unwrap().len(), 0);
    let count = bare.update(|cx| cx.key_bindings().borrow().bindings().count());
    assert_eq!(count, 0);
}

/// An editor inside the workbench's key context.
struct Shell {
    editor: Entity<EditorView>,
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("workbench")
            .size_full()
            .child(self.editor.clone())
    }
}

#[gpui::test]
fn choosing_an_item_runs_in_the_focused_context(
    cx: &mut TestAppContext,
    windowless: &mut TestAppContext,
) {
    install(cx);
    let seen = record(cx);
    let (shell, cx) = cx.add_window_view(|_, cx| Shell {
        editor: cx.new(|cx| {
            let mut model = EditorModel::new(Buffer::new("abc"), None);
            model.set_cursor(0);
            EditorView::new(model, EditorTheme::default_dark(cx), cx)
        }),
    });
    let editor = shell.update(cx, |shell, _| shell.editor.clone());
    // Menu items run in the active window, as AppKit's do.
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&editor.focus_handle(cx), cx);
    });
    cx.run_until_parked();

    cx.dispatch_action(command("Edit", "editor.select_all"));
    let selection = editor.update(cx, |view, _| view.model().selections()[0].range.clone());
    assert_eq!(selection, 0..3);
    let workbench_editor = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(ContextFrame::new("editor"));
    let select_all = Invocation::of("editor.select_all")
        .unwrap()
        .source(Source::Menu)
        .context(workbench_editor);
    assert_eq!(std::mem::take(&mut *seen.borrow_mut()), [select_all]);

    let fullscreen = command("View", "window.toggle_fullscreen");
    assert!(!cx.update(|window, _| window.is_fullscreen()));
    cx.dispatch_action(fullscreen.clone());
    assert!(cx.update(|window, _| window.is_fullscreen()));
    cx.dispatch_action(fullscreen.clone());
    assert!(!cx.update(|window, _| window.is_fullscreen()));

    // With no window, through the app's commands.
    install(windowless);
    let seen = record(windowless);
    windowless.update(|cx| cx.dispatch_action(&fullscreen));
    let toggle = Invocation::of("window.toggle_fullscreen")
        .unwrap()
        .source(Source::Menu);
    assert_eq!(*seen.borrow(), [toggle]);
}
