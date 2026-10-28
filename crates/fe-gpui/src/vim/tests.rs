//! Vim mode in a headless window: keys through the interceptor and the
//! editor's key listener, the switch, the default keymap's vim layer and
//! the block cursor.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, Point};
use fe_command::{ContextFrame, ContextStack, Invocation, Source, Value};
use fe_editor::EditorModel;
use fe_keymap::{
    DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, ItemKind, Keymap, KeymapLayer, KeymapStore, Keystroke,
    Modifiers,
};
use gpui::{Bounds, Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext, px, size};

use super::*;
use crate::{
    EditorTheme, EditorView,
    commands::{RunCommand, command_catalog, observe_invocations},
    init,
    keymap::{CHORD_TIMEOUT, context_stack, set_user_keymap},
    menu,
};

/// `rows` rows of `row N`.
fn rows(rows: u32) -> String {
    (0..rows)
        .map(|row| format!("row {row}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A focused editor showing `text` with a caret at byte `caret`, and every
/// invocation run from then on.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    caret: usize,
) -> (
    Entity<EditorView>,
    &'a mut VisualTestContext,
    Rc<RefCell<Vec<Invocation>>>,
) {
    cx.update(init);
    let seen = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        let sink = seen.clone();
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        });
    });
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_cursor(caret);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    (view, cx, seen)
}

fn set_vim(on: bool, cx: &mut VisualTestContext) {
    cx.update(|_, cx| set_vim_enabled(cx, on));
    cx.run_until_parked();
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

/// The primary caret's head, as a row and a byte column.
fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, u32) {
    view.read_with(cx, |view, _| {
        let model = view.model();
        let point = model
            .text()
            .offset_to_point(model.primary_selection().head());
        (point.row, point.column)
    })
}

fn set_caret(view: &Entity<EditorView>, at: (u32, u32), cx: &mut VisualTestContext) {
    view.update(cx, |view, _| {
        let model = view.model_mut();
        let offset = model.text().point_to_offset(Point::new(at.0, at.1));
        model.set_cursor(offset);
    });
}

/// The context a keystroke would be matched in now.
fn live_context(cx: &mut VisualTestContext) -> ContextFrame {
    cx.update(|window, cx| {
        let stack = context_stack(&window.context_stack());
        let (stack, _) = live_stack(stack, window, cx);
        editor_frame(&stack)
    })
}

fn editor_frame(stack: &ContextStack) -> ContextFrame {
    stack
        .frames()
        .iter()
        .rfind(|frame| frame.name == "editor")
        .expect("an editor frame")
        .clone()
}

/// The vim keys of an editor frame: `vim.mode`, whether `vim.pending` is
/// set, and `vim.pending_kind`.
fn vim_keys(frame: &ContextFrame) -> (Option<&str>, bool, Option<&str>) {
    (
        frame.values.get("vim.mode").map(String::as_str),
        frame.flags.contains("vim.pending"),
        frame.values.get("vim.pending_kind").map(String::as_str),
    )
}

fn run(invocation: Invocation, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
}

#[gpui::test]
fn normal_mode_keys_go_to_fe_vim(cx: &mut TestAppContext) {
    let start = rows(10);
    let (view, cx, seen) = editor(cx, &start, 0);
    set_vim(true, cx);
    seen.borrow_mut().clear();

    cx.simulate_keystrokes("5 j");
    assert_eq!(caret(&view, cx), (5, 0));
    let seen = std::mem::take(&mut *seen.borrow_mut());
    assert_eq!(seen.len(), 1, "{seen:?}");
    let moved = &seen[0];
    assert_eq!(moved.command.as_str(), "vim.move");
    assert_eq!(
        moved.args,
        serde_json::json!({"motion": "down", "extend": false, "count": 5})
    );
    assert_eq!(moved.source, Source::Keyboard);
    assert_eq!(vim_keys(&editor_frame(&moved.context)).0, Some("normal"));

    cx.simulate_keystrokes("down");
    assert_eq!(caret(&view, cx), (6, 0));

    // `editor.move_left` would go back over the line break.
    set_caret(&view, (1, 0), cx);
    cx.simulate_keystrokes("left");
    assert_eq!(caret(&view, cx), (1, 0));

    for keys in [
        "q",
        "shift-q",
        "shift-k",
        "enter",
        "tab",
        "backspace",
        "shift-backspace",
        "delete",
        "alt-backspace",
    ] {
        cx.simulate_keystrokes(keys);
        assert_eq!(text(&view, cx), start, "{keys}");
    }
    cx.simulate_input("QK");
    assert_eq!(text(&view, cx), start);

    cx.simulate_keystrokes("cmd-a");
    let selected = view.read_with(cx, |view, _| view.model().primary_selection().range);
    assert_eq!(selected, 0..start.len());
}

#[gpui::test]
fn pending_input_owns_the_keys(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx, &rows(10), 0);
    set_vim(true, cx);

    // A count typed with no frame drawn since: the drawn context doesn't
    // show it. A key typed with a frame drawn in between draws it, as GPUI
    // draws a dirty window before each key, so the count goes to the
    // state directly, as a key would with no redraw asked for.
    let undrawn = |digit: char, cx: &mut VisualTestContext| {
        view.update(cx, |view, _| {
            view.vim.as_mut().unwrap().feed(VimKey::Char(digit));
        });
        let drawn = cx.update(|window, _| editor_frame(&context_stack(&window.context_stack())));
        assert!(!vim_keys(&drawn).1, "no frame was drawn");
        assert_eq!(vim_keys(&live_context(cx)).2, Some("count"));
    };
    undrawn('2', cx);
    cx.simulate_keystrokes("j");
    assert_eq!(caret(&view, cx), (2, 0));
    // Nor does the keymap run `alt-z` over it.
    undrawn('3', cx);
    cx.simulate_keystrokes("alt-z");
    assert!(!view.read_with(cx, |view, _| view.model().soft_wrap()));
    cx.simulate_keystrokes("j");
    assert_eq!(caret(&view, cx), (3, 0));

    cx.simulate_keystrokes("d");
    assert_eq!(
        vim_keys(&live_context(cx)),
        (Some("operator_pending"), true, Some("operator"))
    );
    cx.simulate_keystrokes("escape");
    assert_eq!(vim_keys(&live_context(cx)), (Some("normal"), false, None));

    cx.simulate_keystrokes("3");
    // Swallowed: on macOS a cmd key that goes on reaches the menu bar.
    let handled = cx.update(|window, cx| {
        window.dispatch_keystroke(gpui::Keystroke::parse("cmd-a").unwrap(), cx)
    });
    assert!(handled);
    let selection = view.read_with(cx, |view, _| view.model().primary_selection().range);
    assert!(selection.is_empty());
    cx.simulate_keystrokes("j");
    assert_eq!(caret(&view, cx), (4, 0));

    let before = view.read_with(cx, |view, _| {
        (view.model().text().to_string(), view.model().selections())
    });
    cx.simulate_keystrokes("escape");
    let after = view.read_with(cx, |view, _| {
        (view.model().text().to_string(), view.model().selections())
    });
    assert_eq!(after, before);
}

#[gpui::test]
fn setting_and_toggle(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx, &rows(10), 0);
    assert!(!cx.update(|_, cx| vim_enabled(cx)));
    cx.simulate_keystrokes("j");
    assert_eq!(text(&view, cx), format!("j{}", rows(10)));
    let stack = cx.update(|window, _| context_stack(&window.context_stack()));
    assert!(
        stack
            .frames()
            .iter()
            .all(|frame| !frame.values.contains_key("vim.mode")),
        "{stack:?}"
    );

    set_vim(true, cx);
    set_caret(&view, (0, 0), cx);
    cx.simulate_keystrokes("j");
    assert_eq!(caret(&view, cx), (1, 0));

    let toggle = Invocation::of(TOGGLE).unwrap();
    run(toggle.clone(), cx);
    assert!(!cx.update(|_, cx| vim_enabled(cx)));
    cx.simulate_keystrokes("j");
    assert_eq!(text(&view, cx).lines().nth(1), Some("jrow 1"));

    // Off and on again drops the pending count.
    run(toggle, cx);
    set_caret(&view, (0, 0), cx);
    cx.simulate_keystrokes("3");
    cx.update(|_, cx| {
        set_vim_enabled(cx, false);
        set_vim_enabled(cx, true);
    });
    cx.simulate_keystrokes("j");
    assert_eq!(caret(&view, cx), (1, 0));

    // With vim off, `vim.move` does nothing.
    set_vim(false, cx);
    let before = text(&view, cx);
    run(
        Invocation::of("vim.move")
            .unwrap()
            .args(Value::from_iter([("motion", "down")])),
        cx,
    );
    assert_eq!((text(&view, cx), caret(&view, cx)), (before, (1, 0)));

    // The View menu's item runs `vim.toggle`.
    let catalog = command_catalog();
    // fe-workbench's items are dropped: their commands are in its catalog.
    let (bar, diagnostics) = fe_keymap::default_menus_macos(Some(&*catalog));
    assert!(
        diagnostics.iter().all(|d| !d.message.contains("vim.")),
        "{diagnostics:?}"
    );
    let bar = bar.unwrap();
    let items: Vec<_> = bar.menu("View").unwrap().items.iter().collect();
    let toggle = items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Command(command) if command.title == "Toggle Vim Mode" => Some(command),
            _ => None,
        })
        .expect("View has Toggle Vim Mode");
    assert_eq!(toggle.command.as_str(), TOGGLE);
    cx.update(|_, cx| menu::install(cx, bar.clone()));
    let chosen = menu::MenuCommand {
        menu: "View".into(),
        item: toggle.id.clone(),
    };
    cx.update(|window, cx| window.dispatch_action(Box::new(chosen), cx));
    cx.run_until_parked();
    assert!(cx.update(|_, cx| vim_enabled(cx)));
}

/// Keys are matched against the default keymap with `user` over it.
fn set_user_layer(user: &str, cx: &mut VisualTestContext) {
    let path = std::path::Path::new(DEFAULT_KEYMAP_PATH);
    let (default, _) = KeymapLayer::parse(path, DEFAULT_MACOS, None);
    let (user, diagnostics) = KeymapLayer::parse(std::path::Path::new("keymap.toml"), user, None);
    assert_eq!(diagnostics, []);
    let keymap = Keymap::from_layers([default.unwrap(), user.unwrap()]);
    cx.update(|_, cx| set_user_keymap(cx, keymap));
}

fn time_out(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(CHORD_TIMEOUT * 2);
    cx.run_until_parked();
}

/// A chord that fails in a vim editor replays its keystrokes to the
/// reducer, every one, before the keystroke that failed it, which goes to
/// vim if they leave a command pending.
#[gpui::test]
fn failed_chords_replay_to_vim(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx, &rows(10), 0);
    set_vim(true, cx);
    let bind = |keys: &str| {
        format!(
            "[[bindings]]\nkeys = \"{keys}\"\ncommand = \"editor.select_all\"\ncontext = \"editor\"\n"
        )
    };
    let user: String = ["3 x", "d x", "k x", "g x", "3 escape x"]
        .map(bind)
        .concat();
    set_user_layer(&user, cx);
    let soft_wrap =
        |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.model().soft_wrap());
    let pending = |cx: &mut VisualTestContext| vim_keys(&live_context(cx)).1;

    // A replayed count or operator owns the shortcut that ends the chord.
    cx.simulate_keystrokes("3 alt-z");
    assert!(!soft_wrap(cx));
    cx.simulate_keystrokes("j");
    assert_eq!(caret(&view, cx), (1, 0));
    cx.simulate_keystrokes("d alt-z");
    assert!(!soft_wrap(cx));
    assert_eq!(vim_keys(&live_context(cx)), (Some("normal"), false, None));
    assert_eq!(text(&view, cx), rows(10));

    // The key ending the chord reaches vim after a replay that commits or
    // waits: k j moves up and down, g g goes to the first row.
    cx.simulate_keystrokes("k j");
    assert_eq!(caret(&view, cx), (1, 0));
    cx.simulate_keystrokes("g g");
    assert_eq!(caret(&view, cx), (0, 0));
    assert!(!pending(cx));
    // A key vim has no name for, after a replay that waits, cancels.
    cx.simulate_keystrokes("g f21");
    assert!(!pending(cx));

    // At a timeout, escape replays too, and cancels the count.
    cx.simulate_keystrokes("3 escape");
    time_out(cx);
    assert!(!pending(cx));
    cx.simulate_keystrokes("j");
    assert_eq!(caret(&view, cx), (1, 0));
    assert_eq!(text(&view, cx), rows(10));
}

/// A chord bound to `vim.key` feeds the reducer each of its keystrokes,
/// once, whether it completes or times out.
#[gpui::test]
fn chords_to_vim_key_feed_every_keystroke(cx: &mut TestAppContext) {
    let (view, cx, seen) = editor(cx, &rows(10), 0);
    set_vim(true, cx);
    set_user_layer(
        r#"
        [[bindings]]
        keys = "3 j"
        command = "vim.key"
        context = "editor && vim.mode == normal"

        [[bindings]]
        keys = "2 j"
        command = "vim.key"
        context = "editor && vim.mode == normal"

        [[bindings]]
        keys = "2 j x"
        command = "editor.select_all"
        context = "editor"
        "#,
        cx,
    );
    seen.borrow_mut().clear();
    cx.simulate_keystrokes("3 j");
    assert_eq!(caret(&view, cx), (3, 0));
    let moves: Vec<_> = seen
        .borrow()
        .iter()
        .map(|seen| seen.command.to_string())
        .collect();
    assert_eq!(moves, ["vim.move"]);
    cx.simulate_keystrokes("2 j");
    time_out(cx);
    assert_eq!(caret(&view, cx), (5, 0));
}

#[gpui::test]
fn vim_layer_of_the_default_keymap(_: &mut TestAppContext) {
    let store = KeymapStore::load(DEFAULT_MACOS, None, None);
    assert_eq!(store.diagnostics(), []);
    let bindings = store.keymap().bindings();
    // Against fe-gpui's catalog, the only bindings dropped are
    // fe-workbench's, whose commands are in its own.
    let checked = KeymapStore::load(DEFAULT_MACOS, None, Some(command_catalog()));
    for diagnostic in checked.diagnostics() {
        let key = diagnostic.key.as_deref().unwrap_or_default();
        let index: usize = key
            .strip_prefix("bindings[")
            .and_then(|rest| rest.strip_suffix("].command"))
            .and_then(|index| index.parse().ok())
            .unwrap_or_else(|| panic!("{diagnostic:?}"));
        let context = bindings[index].context.as_ref().map(ToString::to_string);
        assert!(
            crate::keymap_tests::is_window_key(&bindings[index])
                || bindings[index].command.as_str().starts_with("git.")
                || bindings[index].command.as_str().starts_with("chat.")
                || bindings[index].command.as_str().starts_with("peek.")
                || matches!(
                    context.as_deref(),
                    Some(
                        "workbench"
                            | "workbench && !terminal"
                            | "zen_mode"
                            | "terminal"
                            | "picker"
                            | "text_input"
                            | "search_panel"
                            | "search_panel && text_input"
                            | "find_bar"
                            | "find_bar && text_input"
                            | "editor || find_bar"
                            | "project_panel"
                            | "ReferencesPanel"
                            | "ReferencesPanel && vim"
                            | "search_results"
                            | "search_results && vim"
                            | "project_panel && vim"
                            | "problems_panel"
                            | "ai_changes"
                            | "problems_panel && vim"
                            | "outline_panel"
                            | "outline_panel && vim"
                            | "find_bar && replace_input && text_input"
                            | "editor && vim.mode == normal && !vim.pending"
                            | "editor && vim.mode == normal && !vim.pending && !vim.replace_with_register"
                            | "editor && vim.visual && !vim.replace_with_register"
                            | "editor && vim.visual"
                            | "workbench && !terminal && !vim_line && !(editor && vim.enabled)"
                            | "vim_line"
                            | "vim_line && text_input"
                            | "inline_edit"
                            | "inline_edit && text_input"
                            | "editor"
                            | "editor && (!vim.enabled || vim.mode == insert || vim.mode == replace)"
                            | "(project_panel || outline_panel || ReferencesPanel || problems_panel || search_results || git_panel) && vim && !editor && !text_input"
                            | "(project_panel || outline_panel || ReferencesPanel || problems_panel || search_results || git_panel || terminal_panel) && vim && !editor && !text_input"
                    )
                ),
            "{diagnostic:?}"
        );
    }
    let layer: Vec<_> = bindings
        .iter()
        .filter(|binding| {
            binding.context.as_ref().map(ToString::to_string).as_deref()
                == Some("editor && vim.mode == normal")
                && !crate::keymap_tests::is_window_key(binding)
        })
        .map(|binding| {
            (
                binding.keys.to_string(),
                binding.command.as_str(),
                binding.args.clone(),
            )
        })
        .collect();
    let keys = |keys: &str| fe_keymap::KeySequence::parse(keys).unwrap().to_string();
    let mut expected: Vec<_> = ["left", "right", "up", "down"]
        .map(|motion| {
            (
                keys(motion),
                "vim.move",
                serde_json::json!({ "motion": motion }),
            )
        })
        .into();
    for key in [
        "enter",
        "tab",
        "backspace",
        "shift-backspace",
        "delete",
        "alt-backspace",
    ] {
        expected.push((keys(key), KEY, Value::Null));
    }
    assert_eq!(layer, expected);

    // No ctrl key without cmd applies in a vim editor: they're all vim's,
    // but for the workbench's Open Editor at Index 1 to 9 (ctrl-1 to ctrl-9,
    // keys vim leaves unbound), Go Back and Go Forward (ctrl-minus and
    // ctrl-shift-minus), Toggle Terminal (ctrl-`) and New Terminal
    // (ctrl-shift-`), which vim doesn't bind, and `ctrl-space`, which asks for completions only where text is
    // typed: in insert mode, and Show Changes (ctrl-shift-g), which the
    // reducer reads as `<C-g>`, a key vim leaves unbound, and `ctrl-]`, which
    // goes to the definition as nvim's tag jump does.
    let stack = ContextStack::new()
        .with(ContextFrame::new("workbench"))
        .with(
            ContextFrame::new("editor")
                .flag("vim.enabled")
                .set("vim.mode", "normal"),
        );
    let ctrl: Vec<_> = bindings
        .iter()
        .filter(|binding| {
            binding
                .keys
                .keystrokes()
                .iter()
                .any(|keystroke: &Keystroke| {
                    let Modifiers { cmd, ctrl, .. } = keystroke.modifiers;
                    ctrl && !cmd
                })
        })
        .filter(|binding| binding.depth(&stack).is_some())
        // Vim's `<C-w>` window keys run the pane commands.
        .filter(|binding| !crate::keymap_tests::is_window_key(binding))
        .map(|binding| binding.keys.to_string())
        .collect();
    assert_eq!(
        ctrl,
        [
            "ctrl-shift-r",
            "ctrl-1",
            "ctrl-2",
            "ctrl-3",
            "ctrl-4",
            "ctrl-5",
            "ctrl-6",
            "ctrl-7",
            "ctrl-8",
            "ctrl-9",
            "ctrl--",
            "ctrl-shift--",
            "ctrl-`",
            "ctrl-shift-`",
            "ctrl-space",
            "ctrl-g",
            "ctrl-shift-g",
            "ctrl-]"
        ]
    );
}

#[gpui::test]
fn block_cursor_in_normal_mode(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx, "a漢b\n\nxy", 1);
    let draw = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        view.read_with(cx, |view, _| view.painted_carets.clone())
    };
    let bar_at = |offset: usize, cx: &mut VisualTestContext| {
        view.update(cx, |view, _| {
            view.layout.as_mut().unwrap().caret_bounds(offset).unwrap()
        })
    };
    let one = |bounds: &Bounds<Pixels>, to_x: Pixels| {
        vec![Bounds::new(
            bounds.origin,
            size(to_x - bounds.origin.x, bounds.size.height),
        )]
    };

    // With vim off, a bar.
    let painted = draw(cx);
    assert_eq!(painted, [bar_at(1, cx)]);
    assert_eq!(painted[0].size.width, px(2.));

    set_vim(true, cx);
    let painted = draw(cx);
    let (at_1, at_4) = (bar_at(1, cx), bar_at(4, cx));
    assert_eq!(painted, one(&at_1, at_4.origin.x));

    // An empty row: one space.
    set_caret(&view, (1, 0), cx);
    let painted = draw(cx);
    let space = cx.update(|_, cx| {
        let theme = view.read(cx).theme.clone();
        let text_system = cx.text_system();
        let font = text_system.resolve_font(&gpui::font(theme.font_family));
        text_system.layout_width(font, theme.font_size, ' ')
    });
    let at_6 = bar_at(6, cx);
    assert_eq!(painted.len(), 1);
    assert_eq!(painted[0].origin, at_6.origin);
    assert_eq!(painted[0].size.height, at_6.size.height);
    assert!(
        (painted[0].size.width - space).abs() < px(0.01),
        "{painted:?}"
    );
    assert!(space > px(2.));

    // A grapheme of two chars, `é` as `e` and a combining accent.
    view.update(cx, |view, _| {
        let model = view.model_mut();
        model.set_cursor(6);
        model.insert("e\u{301}z");
        model.set_cursor(6);
    });
    let painted = draw(cx);
    let (at_6, at_9) = (bar_at(6, cx), bar_at(9, cx));
    assert_eq!(painted, one(&at_6, at_9.origin.x));

    cx.update(|window, cx| window.blur(cx));
    assert_eq!(draw(cx), []);
}

#[gpui::test]
fn block_cursor_before_a_soft_wrap(cx: &mut TestAppContext) {
    // Tabs three columns wide, so a block one space wide would be too
    // narrow.
    let long = "x\t".repeat(1000);
    let (view, cx, _) = editor(cx, &long, 0);
    set_vim(true, cx);
    view.update(cx, |view, _| view.model_mut().set_soft_wrap(true));
    let block_at = |offset: usize, cx: &mut VisualTestContext| {
        view.update(cx, |view, _| view.model_mut().set_cursor(offset));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        view.read_with(cx, |view, _| view.painted_carets.clone())
    };
    block_at(0, cx);
    let wrap = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.rows[0].end
    });
    assert!(wrap < long.len(), "the row wraps");
    assert_eq!(&long[wrap - 1..wrap], "\t");
    // The tab before the wrap is as wide as the row's first, though a caret
    // at the wrap point sits on the next row.
    let before = block_at(1, cx);
    let last = block_at(wrap - 1, cx);
    assert_eq!(last.len(), 1);
    assert_eq!(last[0].origin.y, before[0].origin.y);
    assert!(before[0].size.width > px(20.), "{before:?}");
    assert!(
        (last[0].size.width - before[0].size.width).abs() < px(0.01),
        "{last:?} {before:?}"
    );
}

mod macro_replay;

/// A focused vim editor on `source`'s keymap, with every invocation seen.
fn platform_editor<'a>(
    cx: &'a mut TestAppContext,
    source: &str,
    text: &str,
) -> (
    Entity<EditorView>,
    &'a mut VisualTestContext,
    Rc<RefCell<Vec<Invocation>>>,
) {
    let path = std::path::Path::new(DEFAULT_KEYMAP_PATH);
    let (layer, diagnostics) = KeymapLayer::parse(path, source, None);
    assert_eq!(diagnostics, []);
    let map = Keymap::from_layers([layer.unwrap()]);
    cx.update(|cx| crate::init_with_keymap(cx, map));
    let seen = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        let sink = seen.clone();
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        });
    });
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new(&text), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    set_vim(true, cx);
    (view, cx, seen)
}

/// The keys vim's handler reads as tab and the arrows reach vim through the
/// listener, not an editor command, on both platform keymaps.
#[gpui::test]
fn modified_keys_reach_vim_through_key_input(cx: &mut TestAppContext) {
    for (os, source, duplicate) in [
        ("macOS", DEFAULT_MACOS, "shift-alt-down"),
        ("Linux", fe_keymap::DEFAULT_LINUX, "ctrl-shift-alt-down"),
    ] {
        let (_, cx, seen) = platform_editor(cx, source, &rows(6));
        seen.borrow_mut().clear();
        cx.simulate_keystrokes("shift-tab");
        let commands: Vec<_> = seen
            .borrow()
            .iter()
            .map(|invocation| invocation.command.as_str().to_owned())
            .collect();
        assert_eq!(commands, ["vim.jump_newer"], "{os}");

        for (mode_keys, mode) in [
            ("v", "visual"),
            ("V", "visual_line"),
            ("ctrl-v", "visual_block"),
        ] {
            let (view, cx, seen) = platform_editor(cx, source, &rows(6));
            cx.simulate_keystrokes(mode_keys);
            let start = caret(&view, cx);
            for keys in ["alt-down", duplicate] {
                seen.borrow_mut().clear();
                cx.simulate_keystrokes(keys);
                assert!(
                    seen.borrow()
                        .iter()
                        .all(|i| i.command.as_str().starts_with("vim.")),
                    "{os} {mode} {keys}: {:?}",
                    seen.borrow()
                );
                assert!(!seen.borrow().is_empty(), "{os} {mode} {keys}");
            }
            let moved = caret(&view, cx);
            assert_eq!(moved.0, start.0 + 2, "{os} {mode}");
            cx.simulate_keystrokes("alt-up");
            assert_eq!(caret(&view, cx).0, start.0 + 1, "{os} {mode}");
        }
    }
}
