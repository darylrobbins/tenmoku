//! Remaps run in the editor: a map's keys in the keystroke that completes
//! it, a held prefix owning the next keystroke until [`CHORD_TIMEOUT`],
//! and the ex lines a map runs or leaves open.

use fe_buffer::{Buffer, Point};
use fe_command::Invocation;
use fe_editor::EditorModel;
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, Keymap, KeymapLayer};
use fe_vim::{HostCommand, Remap, RemapMode};
use gpui::{
    Entity, EntityInputHandler as _, Focusable as _, Modifiers, Pixels, TestAppContext,
    VisualTestContext,
};

use super::*;
use crate::{
    EditorTheme, EditorView,
    commands::RunCommand,
    init,
    keymap::{context_stack, set_user_keymap},
};

/// A focused editor in a window of its own showing `text`, vim on, the
/// caret at `at`, and `remaps` set with `,` as the leader.
fn vim_editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    at: (u32, u32),
    remaps: Vec<Remap>,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        let offset = model.text().point_to_offset(Point::new(at.0, at.1));
        model.set_cursor(offset);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    // Keys come to an editor focused in the active window.
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    cx.run_until_parked();
    (view, cx)
}

fn noremap(mode: RemapMode, from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![mode],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    }
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

fn mode(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    view.read_with(cx, |view, _| view.vim_mode())
}

/// Whether the editor owns the next keystroke, as [`live_stack`] says, and
/// `vim.pending_kind` on its live frame.
fn pending(cx: &mut VisualTestContext) -> (bool, Option<String>) {
    cx.update(|window, cx| {
        let stack = context_stack(&window.context_stack());
        let (stack, owned) = live_stack(stack, window, cx);
        let frame = stack
            .frames()
            .iter()
            .rfind(|frame| frame.name == "editor")
            .expect("an editor frame")
            .clone();
        (owned, frame.values.get("vim.pending_kind").cloned())
    })
}

fn time_out(cx: &mut VisualTestContext) {
    cx.executor().advance_clock(CHORD_TIMEOUT * 2);
    cx.run_until_parked();
}

fn position_of(
    view: &Entity<EditorView>,
    offset: usize,
    cx: &mut VisualTestContext,
) -> gpui::Point<Pixels> {
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        view.update(cx, |view, cx| {
            let utf16 = view.model().text().offset_to_utf16(offset);
            view.bounds_for_range(utf16..utf16, Default::default(), window, cx)
                .expect("visible")
                .center()
        })
    })
}

const INDENTED: &str = "  one two\n  three four\n  five six";

/// `nnoremap H ^` moves the caret in the keystroke `H`, after a count too,
/// and `onoremap H ^` after an operator.
#[gpui::test]
fn expansion_runs_on_its_key(cx: &mut TestAppContext) {
    let remaps = vec![
        noremap(RemapMode::Normal, "H", "^"),
        noremap(RemapMode::OperatorPending, "H", "^"),
    ];
    let (view, cx) = vim_editor(cx, INDENTED, (1, 6), remaps);
    cx.simulate_keystrokes("shift-h");
    assert_eq!(caret(&view, cx), (1, 2));
    assert_eq!(pending(cx), (false, None));

    // `^` takes the count and ignores it, where `3H` unmapped would go to
    // the window's third row.
    cx.simulate_keystrokes("$ 3 shift-h");
    assert_eq!(caret(&view, cx), (1, 2));
    assert_eq!(pending(cx), (false, None));

    cx.simulate_keystrokes("4 l d shift-h");
    assert_eq!(text(&view, cx), "  one two\n  e four\n  five six");
    assert_eq!(caret(&view, cx), (1, 2));
    assert_eq!(mode(&view, cx), Some("normal"));
    // As a change `.` repeats.
    cx.simulate_keystrokes("j $ .");
    assert_eq!(text(&view, cx), "  one two\n  e four\n  x");
}

/// With `gb` and `gbc` mapped, `g` holds: the next key is vim's even where
/// the keymap binds it, until `CHORD_TIMEOUT` passes with no key, which
/// resolves what's held as [`VimState::remap_timeout`] does, or a click.
#[gpui::test]
fn prefix_owns_next_key(cx: &mut TestAppContext) {
    let remaps = vec![
        noremap(RemapMode::Normal, "gb", "x"),
        noremap(RemapMode::Normal, "gbc", "dd"),
    ];
    let (view, cx) = vim_editor(cx, "abc\ndef\nghi", (2, 1), remaps);
    let path = std::path::Path::new(DEFAULT_KEYMAP_PATH);
    let (default, _) = KeymapLayer::parse(path, DEFAULT_MACOS, None);
    let user =
        "[[bindings]]\nkeys = \"b\"\ncommand = \"editor.select_all\"\ncontext = \"editor\"\n";
    let (user, diagnostics) = KeymapLayer::parse(std::path::Path::new("keymap.toml"), user, None);
    assert_eq!(diagnostics, []);
    let keymap = Keymap::from_layers([default.unwrap(), user.unwrap()]);
    cx.update(|_, cx| set_user_keymap(cx, keymap));
    let selected = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            !view.model().primary_selection().range.is_empty()
        })
    };
    // `g` starts the default keymap's chords (`g d`): the keymap hands it
    // to vim once they time out.
    let to_vim = |cx: &mut VisualTestContext| {
        cx.simulate_keystrokes("g");
        cx.executor().advance_clock(CHORD_TIMEOUT);
        cx.run_until_parked();
        assert_eq!(pending(cx), (true, Some("remap".into())));
    };

    // Unheld, `b` is the keymap's.
    cx.simulate_keystrokes("b");
    assert!(selected(cx));
    view.update(cx, |view, _| {
        let offset = view.model().text().point_to_offset(Point::new(2, 1));
        view.model_mut().set_cursor(offset);
    });
    assert!(!selected(cx));

    // Held, it's vim's. `gb` is complete but `gbc` longer: it waits again,
    // `CHORD_TIMEOUT` from `b`, not from `g`, then `gb` runs.
    to_vim(cx);
    cx.executor().advance_clock(CHORD_TIMEOUT / 2);
    cx.simulate_keystrokes("b");
    assert!(!selected(cx));
    assert_eq!(pending(cx), (true, Some("remap".into())));
    cx.executor().advance_clock(CHORD_TIMEOUT / 2);
    cx.run_until_parked();
    assert_eq!(pending(cx), (true, Some("remap".into())));
    assert_eq!(text(&view, cx), "abc\ndef\nghi");
    cx.executor().advance_clock(CHORD_TIMEOUT);
    cx.run_until_parked();
    assert_eq!(text(&view, cx), "abc\ndef\ngi");
    assert_eq!(pending(cx), (false, None));

    // Typed through, `gbc` runs in the keystroke `c`.
    to_vim(cx);
    cx.simulate_keystrokes("b c");
    assert_eq!(text(&view, cx), "abc\ndef");
    assert_eq!(pending(cx), (false, None));

    // At the timeout `g` goes on as typed, unmapped: a `g` prefix, which
    // a second `g` completes.
    cx.simulate_keystrokes("g");
    time_out(cx);
    let (owned, kind) = pending(cx);
    assert!(owned);
    assert!(
        kind.is_some() && kind.as_deref() != Some("remap"),
        "{kind:?}"
    );
    cx.simulate_keystrokes("g");
    assert_eq!(caret(&view, cx).0, 0);
    assert_eq!(pending(cx), (false, None));

    // A click resolves the held `g` and drops the prefix it leaves.
    let at = position_of(&view, 5, cx);
    to_vim(cx);
    cx.simulate_click(at, Modifiers::none());
    assert_eq!(pending(cx), (false, None));
    assert_eq!(caret(&view, cx), (1, 1));
    time_out(cx);
    assert_eq!(text(&view, cx), "abc\ndef");
    assert_eq!(pending(cx), (false, None));
    cx.simulate_keystrokes("b");
    assert!(selected(cx));

    // Losing focus resolves it too, as the timeout would: `g` goes on.
    view.update(cx, |view, _| view.model_mut().set_cursor(0));
    to_vim(cx);
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let state = view.vim_state().unwrap();
        assert!(!state.remap_waiting());
        assert!(state.pending().kind().is_some());
    });
}

/// A map's `:w<CR>` leaves the write for the workbench, where a macro
/// replay's is dropped.
#[gpui::test]
fn mapped_write_leaves_host_command(cx: &mut TestAppContext) {
    let remaps = vec![noremap(RemapMode::Normal, "<leader>w", ":w<CR>")];
    let (view, cx) = vim_editor(cx, "abc", (0, 0), remaps);
    let host_command =
        |cx: &mut VisualTestContext| view.update(cx, |view, _| view.take_vim_host_command());
    cx.simulate_keystrokes(", w");
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(
        host_command(cx),
        Some(HostCommand::Write {
            force: false,
            path: None,
        })
    );

    // A macro recording `:w<CR>` typed on the host's line.
    cx.simulate_keystrokes("q a :");
    assert_eq!(mode(&view, cx), Some("cmdline"));
    view.update(cx, |view, _| view.record_command_line("w", VimKey::Enter));
    let submit = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": "w" }));
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(submit)), cx));
    cx.run_until_parked();
    cx.simulate_keystrokes("q");
    assert!(host_command(cx).is_some());
    cx.simulate_keystrokes("@ a");
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(host_command(cx), None);
}

/// A map that leaves the command line open puts its text on the host's
/// line.
#[gpui::test]
fn open_line_keeps_text(cx: &mut TestAppContext) {
    let remaps = vec![noremap(RemapMode::Normal, "<leader>s", ":%s/")];
    let (view, cx) = vim_editor(cx, "abc", (0, 0), remaps);
    cx.simulate_keystrokes(", s");
    assert_eq!(mode(&view, cx), Some("cmdline"));
    let prefill = view.read_with(cx, |view, _| {
        view.vim_state().unwrap().command_line_prefill().to_owned()
    });
    assert_eq!(prefill, "%s/");
}

fn host_commands(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<HostCommand> {
    view.update(cx, |view, _| {
        std::iter::from_fn(|| view.take_vim_host_command()).collect()
    })
}

/// A map's ex line that leaves a file or window command ends the map: the
/// host runs that command once the keystroke is over, so nothing after it
/// runs before it, or with the host's answer unknown; the editor says what
/// it left out.
#[gpui::test]
fn a_mapped_host_command_ends_the_map(cx: &mut TestAppContext) {
    let remaps = vec![
        noremap(RemapMode::Normal, "<leader>w", ":w<CR>"),
        noremap(RemapMode::Normal, "<leader>n", ":w<CR>:nohlsearch<CR>"),
        noremap(RemapMode::Normal, "<leader>q", ":w<CR>:q<CR>"),
        noremap(RemapMode::Normal, "<leader>d", ":w<CR>dd"),
        noremap(RemapMode::Normal, "<leader>g", "do:w<CR>"),
    ];
    let (view, cx) = vim_editor(cx, "abc\ndef", (0, 0), remaps);
    let write = HostCommand::Write {
        force: false,
        path: None,
    };
    let message = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.vim_state().unwrap().message().map(str::to_owned)
        })
    };
    cx.simulate_keystrokes(", w");
    assert_eq!(host_commands(&view, cx), std::slice::from_ref(&write));
    assert_eq!(message(cx), None);
    for keys in [", n", ", q", ", d"] {
        cx.simulate_keystrokes(keys);
        assert_eq!(
            host_commands(&view, cx),
            std::slice::from_ref(&write),
            "{keys}"
        );
        assert_eq!(
            message(cx).as_deref(),
            Some("a map's keys after a file or window command aren't run"),
            "{keys}"
        );
        assert_eq!(text(&view, cx), "abc\ndef", "{keys}");
        assert_eq!(mode(&view, cx), Some("normal"), "{keys}");
        assert_eq!(pending(cx), (false, None), "{keys}");
    }
    // `do` hands the host its `:diffget` and ends the map just so: the
    // write after it isn't run.
    cx.simulate_keystrokes(", g");
    let left = host_commands(&view, cx);
    assert!(
        matches!(left.as_slice(), [HostCommand::DiffGet { .. }]),
        "{left:?}"
    );
    assert_eq!(
        message(cx).as_deref(),
        Some("a map's keys after a file or window command aren't run")
    );
    assert_eq!(mode(&view, cx), Some("normal"));

    // The map is over: keys are typed again.
    cx.simulate_keystrokes("x");
    assert_eq!(text(&view, cx), "bc\ndef");
}

/// A map's arrows are matched against the maps, as its other keys are,
/// and one completes a held prefix; a `noremap`'s arrow moves.
#[gpui::test]
fn mapped_arrows_are_remapped(cx: &mut TestAppContext) {
    let mut recursive = noremap(RemapMode::Normal, "H", "<Right>");
    recursive.recursive = true;
    let mut held = noremap(RemapMode::Normal, "Q", "g<Right>");
    held.recursive = true;
    let remaps = vec![
        recursive,
        held,
        noremap(RemapMode::Normal, "L", "<Right>"),
        noremap(RemapMode::Normal, "<Right>", "x"),
        noremap(RemapMode::Normal, "g<Right>", "~"),
    ];
    let (view, cx) = vim_editor(cx, "abcdef", (0, 0), remaps);
    cx.simulate_keystrokes("shift-l");
    assert_eq!(
        (text(&view, cx), caret(&view, cx)),
        ("abcdef".into(), (0, 1))
    );
    cx.simulate_keystrokes("shift-h");
    assert_eq!(
        (text(&view, cx), caret(&view, cx)),
        ("acdef".into(), (0, 1))
    );
    cx.simulate_keystrokes("shift-q");
    assert_eq!(
        (text(&view, cx), caret(&view, cx)),
        ("aCdef".into(), (0, 2))
    );
    assert_eq!(pending(cx), (false, None));
}

/// A replay the keymap dispatched (`@` as a command) that ends holding keys
/// for a map waits as a typed key's do: until the timeout, or the focus
/// going.
#[gpui::test]
fn dispatched_replay_waits(cx: &mut TestAppContext) {
    let remaps = vec![noremap(RemapMode::Normal, "<leader>b", "x")];
    let (view, cx) = vim_editor(cx, "x,yz", (0, 1), remaps);
    // `,` into register `a`.
    cx.simulate_keystrokes("\" a y l");
    let play = |cx: &mut VisualTestContext| {
        let play = Invocation::of("vim.play")
            .unwrap()
            .args(serde_json::json!({ "register": "a" }));
        cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(play)), cx));
        cx.run_until_parked();
    };
    let waiting = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.vim_state().unwrap().remap_waiting())
    };

    play(cx);
    assert!(waiting(cx));
    time_out(cx);
    assert!(!waiting(cx));

    play(cx);
    assert!(waiting(cx));
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    assert!(!waiting(cx));
    assert_eq!(text(&view, cx), "x,yz");
}

/// Only one editor waits: one starting to resolves another's held keys,
/// even before a frame tells it it lost focus, and keys still held after
/// that resolve too, in an editor of a window that isn't active.
#[gpui::test]
fn one_editor_waits(app: &mut TestAppContext) {
    let remaps = vec![
        noremap(RemapMode::Normal, "<leader>b", "x"),
        noremap(RemapMode::Normal, ",,z", "x"),
        noremap(RemapMode::Normal, ",w", "x"),
    ];
    let (second, _) = vim_editor(app, "abc", (0, 0), remaps);
    let second_window = app.windows()[0];
    let window = app.add_window(|_, cx| {
        EditorView::new(
            EditorModel::new(Buffer::new("abc"), None),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    let first = window.root(app).unwrap();
    let mut first_cx = VisualTestContext::from_window(window.into(), app);
    let mut second_cx = VisualTestContext::from_window(second_window, app);
    let activate = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            window.activate_window();
            window.focus(&editor.focus_handle(cx), cx);
        });
        cx.run_until_parked();
    };
    let waiting = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.vim_state().unwrap().remap_waiting())
    };

    activate(&first, &mut first_cx);
    first_cx.simulate_keystrokes(",");
    assert!(waiting(&first, &mut first_cx));
    activate(&second, &mut second_cx);
    second_cx.simulate_keystrokes(",");
    assert!(waiting(&second, &mut second_cx));
    assert!(!waiting(&first, &mut second_cx));
    second_cx.simulate_keystrokes("b");
    assert_eq!(text(&second, &mut second_cx), "bc");
    assert_eq!(text(&first, &mut second_cx), "abc");

    // `,,` resolves to `,` and `,` held again for `,w`: that resolves in
    // the first editor, its window not the active one, and the second
    // keeps its wait.
    activate(&first, &mut first_cx);
    first_cx.simulate_keystrokes(", ,");
    assert!(waiting(&first, &mut first_cx));
    activate(&second, &mut second_cx);
    second_cx.simulate_keystrokes(",");
    assert!(!waiting(&first, &mut second_cx));
    assert!(waiting(&second, &mut second_cx));
    second_cx.executor().advance_clock(CHORD_TIMEOUT / 2);
    second_cx.run_until_parked();
    assert!(waiting(&second, &mut second_cx));
    second_cx.simulate_keystrokes("w");
    assert_eq!(text(&second, &mut second_cx), "c");
    assert_eq!(text(&first, &mut second_cx), "abc");
}

/// A macro's host command is dropped, even where a map plays the macro;
/// a map's own write before it is left for the host, and ends the map.
#[gpui::test]
fn macro_in_a_map_keeps_the_maps_command(cx: &mut TestAppContext) {
    let remaps = vec![
        noremap(RemapMode::Normal, "H", ":w<CR>@a"),
        noremap(RemapMode::Normal, "L", "@b"),
    ];
    let (view, cx) = vim_editor(cx, "abc", (0, 0), remaps);
    let record = |register: &str, line: &str, cx: &mut VisualTestContext| {
        cx.simulate_keystrokes(&format!("q {register} :"));
        view.update(cx, |view, _| view.record_command_line(line, VimKey::Enter));
        let submit = Invocation::of("vim.command_line_submit")
            .unwrap()
            .args(serde_json::json!({ "text": line }));
        cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(submit)), cx));
        cx.run_until_parked();
        cx.simulate_keystrokes("q");
        host_commands(&view, cx);
    };
    record("a", "nohlsearch", cx);
    record("b", "q!", cx);
    let write = HostCommand::Write {
        force: false,
        path: None,
    };
    cx.simulate_keystrokes("shift-h");
    assert_eq!(host_commands(&view, cx), std::slice::from_ref(&write));
    cx.simulate_keystrokes("shift-l");
    assert_eq!(host_commands(&view, cx), []);
    assert_eq!(mode(&view, cx), Some("normal"));
}
