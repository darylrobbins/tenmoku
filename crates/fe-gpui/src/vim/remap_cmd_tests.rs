//! A remap's `<Cmd>…<CR>` in the editor: `fe:{command id}` dispatched as a
//! keymap binding is, and an ex line's file command left for the workbench.

use std::{cell::RefCell, fs, path::Path, rc::Rc};

use fe_buffer::{Buffer, Point};
use fe_command::{Invocation, Source};
use fe_editor::EditorModel;
use fe_vim::{HostCommand, Remap, RemapMode};
use gpui::{AppContext as _, Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, commands::observe_invocations, init, keymap::context_stack};

/// An app command of the host's own, as the workbench registers them,
/// that does nothing.
const PING: &str = "test.ping";

/// Registers [`PING`].
fn register_ping(cx: &mut VisualTestContext) {
    register_app(cx, PING, |_| {});
}

/// Registers an app command of the host's own, `id`, that runs `run`.
fn register_app(cx: &mut VisualTestContext, id: &'static str, run: impl Fn(&mut App) + 'static) {
    cx.update(|_, cx| {
        kiln_platform::commands::register_app_commands(cx, |registry| {
            registry
                .register(id, id, move |cx: &mut &mut App, _: &fe_command::Actor| {
                    run(cx);
                    Ok(fe_command::Dispatched::new(fe_command::Effect::Nothing))
                })
                .unwrap();
        })
    });
}

/// What read-only says of an edit.
const E21: &str = "E21: Cannot make changes, 'modifiable' is off";

/// A focused editor in a window of its own over `model`, vim on, the caret
/// at the start, and `remaps` set with `,` as the leader.
fn vim_editor(
    cx: &mut TestAppContext,
    model: impl FnOnce() -> EditorModel + 'static,
    remaps: Vec<Remap>,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx
        .add_window_view(move |_, cx| EditorView::new(model(), EditorTheme::default_dark(cx), cx));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.update(|_, cx| set_vim_enabled(cx, true));
    // A shown popup holds its prefix; these tests time prefixes out.
    cx.update(|_, cx| crate::set_vim_which_key(cx, false, std::time::Duration::ZERO));
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    cx.run_until_parked();
    (view, cx)
}

fn opened(path: &Path) -> impl FnOnce() -> EditorModel + 'static {
    let path = path.to_owned();
    move || EditorModel::open(&path).unwrap()
}

fn text_model(text: &str) -> impl FnOnce() -> EditorModel + 'static {
    let text = text.to_owned();
    move || EditorModel::new(Buffer::new(&text), None)
}

fn noremap(from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![RemapMode::Normal, RemapMode::Visual],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    }
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Point {
    view.read_with(cx, |view, _| {
        let model = view.model();
        model
            .text()
            .offset_to_point(model.primary_selection().head())
    })
}

fn message(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| {
        view.vim_state().unwrap().message().map(str::to_owned)
    })
}

/// The invocations of `command` that ran from now on.
fn observed(command: &'static str, cx: &mut VisualTestContext) -> Rc<RefCell<Vec<Invocation>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    cx.update(|_, cx| {
        observe_invocations(cx, move |invocation| {
            if invocation.command.as_str() == command {
                sink.borrow_mut().push(invocation.clone());
            }
        })
    });
    seen
}

#[gpui::test]
fn fe_command_dispatches(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\ntwo\n").unwrap();
    let remaps = vec![
        noremap("<leader>s", "<Cmd>fe:editor.save<CR>x"),
        noremap("<leader>u", "<Cmd>fe:nowhere.unknown<CR>j"),
        noremap("<leader>w", "<Cmd>w<CR>j"),
    ];
    let (view, cx) = vim_editor(cx, opened(&path), remaps);
    let saves = observed("editor.save", cx);

    // The editor saves, from the keyboard, in the window's context stamped
    // as the reducer left the state; then the map goes on, in order.
    cx.simulate_keystrokes(", s");
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\n");
    assert_eq!(text(&view, cx), "ne\ntwo\n");
    let context = cx.update(|window, cx| {
        let state = view.read(cx).vim_state();
        stamp(context_stack(&window.context_stack()), state)
    });
    let save = Invocation::of("editor.save")
        .unwrap()
        .source(Source::Keyboard)
        .context(context);
    assert_eq!(*saves.borrow(), [save]);
    assert_eq!(message(&view, cx), None);

    // From a visual mode too, which it stays in for the `x` after it.
    cx.simulate_keystrokes("v , s");
    cx.run_until_parked();
    assert_eq!(saves.borrow().len(), 2);
    let in_visual = saves.borrow()[1].context.frames().iter().any(|frame| {
        frame.name == "editor" && frame.values.get("vim.mode").map(String::as_str) == Some("visual")
    });
    assert!(in_visual);
    assert_eq!(fs::read_to_string(&path).unwrap(), "ne\ntwo\n");
    assert_eq!(text(&view, cx), "e\ntwo\n");
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("normal")
    );

    // An ID no command has says so, and drops the rest of the map.
    cx.simulate_keystrokes(", u");
    cx.run_until_parked();
    assert_eq!(caret(&view, cx), Point::new(0, 0));
    assert_eq!(
        message(&view, cx).as_deref(),
        Some("E492: Not an editor command: fe:nowhere.unknown")
    );

    // `<Cmd>w<CR>` leaves the write for the workbench, without the command
    // line, and ends the map as a typed `:w` would.
    cx.simulate_keystrokes(", w");
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("normal")
    );
    assert_eq!(
        view.update(cx, |view, _| view.take_vim_host_command()),
        Some(HostCommand::Write {
            force: false,
            path: None,
        })
    );
    assert_eq!(caret(&view, cx), Point::new(0, 0));
    assert_eq!(
        message(&view, cx).as_deref(),
        Some("a map's keys after a file or window command aren't run")
    );
}

/// A command the app runs, not the editor, goes to the focused element as
/// a key's would, and the map goes on after it.
#[gpui::test]
fn app_command_runs_and_the_map_goes_on(cx: &mut TestAppContext) {
    let remaps = vec![
        noremap("<leader>a", "<Cmd>fe:test.ping<CR>j"),
        noremap("<leader>t", "<Cmd>fe:vim.toggle<CR>j"),
    ];
    let (view, cx) = vim_editor(cx, text_model("one\ntwo\nthree"), remaps);
    register_ping(cx);
    let shown = observed(PING, cx);
    cx.simulate_keystrokes(", a");
    cx.run_until_parked();
    assert_eq!(shown.borrow().len(), 1);
    assert_eq!(shown.borrow()[0].source, Source::Keyboard);
    assert_eq!(caret(&view, cx), Point::new(1, 0));
    assert_eq!(message(&view, cx), None);

    // One that turns vim off ends it.
    cx.simulate_keystrokes(", t");
    cx.run_until_parked();
    assert!(!cx.update(|_, cx| vim_enabled(cx)));
    assert_eq!(caret(&view, cx), Point::new(1, 0));
}

/// A map resolved by its timeout runs its command in the window's context
/// too, as a key's binding: `editor.save` saves as `cmd-s` does.
#[gpui::test]
fn timed_out_map_runs_in_the_window(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\ntwo\n").unwrap();
    let remaps = vec![
        noremap("<leader>s", "<Cmd>fe:editor.save<CR>j"),
        noremap("<leader>ss", "x"),
    ];
    let (view, cx) = vim_editor(cx, opened(&path), remaps);
    let saves = observed("editor.save", cx);
    view.update(cx, |view, cx| {
        view.model_mut().insert("new ");
        cx.notify();
    });
    cx.simulate_keystrokes(", s");
    assert!(saves.borrow().is_empty());
    cx.executor()
        .advance_clock(crate::keymap::CHORD_TIMEOUT * 2);
    cx.run_until_parked();
    assert_eq!(fs::read_to_string(&path).unwrap(), "new one\ntwo\n");
    assert_eq!(caret(&view, cx).row, 1);
    let context = cx.update(|window, cx| {
        let state = view.read(cx).vim_state();
        stamp(context_stack(&window.context_stack()), state)
    });
    let save = Invocation::of("editor.save")
        .unwrap()
        .source(Source::Keyboard)
        .context(context);
    assert_eq!(*saves.borrow(), [save]);
}

/// A command that can't run here stops the map with vim's message: one
/// that needs args, one whose context doesn't hold, and an edit read-only
/// refuses. One that hands the workbench a file or window command ends it,
/// as a typed `do` does.
#[gpui::test]
fn failing_commands_stop_the_map(cx: &mut TestAppContext) {
    let remaps = vec![
        noremap("<leader>i", "<Cmd>fe:editor.insert<CR>x"),
        noremap("<leader>v", "<Cmd>fe:vim.visual_exit<CR>x"),
        noremap("<leader>d", "<Cmd>fe:vim.diff_get<CR>x"),
        noremap("<leader>D", "<Cmd>fe:vim.diff_put<CR>x"),
        noremap("<leader>b", "<Cmd>fe:editor.backspace<CR>x"),
        noremap("<leader>h", "<Cmd>noh<CR>j"),
        noremap("<leader>r", "<Cmd>s/o/0/<CR>j"),
    ];
    let (view, cx) = vim_editor(cx, text_model("one\ntwo\nthree"), remaps);
    let unchanged = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        assert_eq!(text(view, cx), "one\ntwo\nthree");
        assert_eq!(caret(view, cx), Point::new(0, 0));
    };

    cx.simulate_keystrokes(", i");
    cx.run_until_parked();
    unchanged(&view, cx);
    let needs = message(&view, cx).unwrap();
    assert!(needs.starts_with("fe:editor.insert: "), "{needs}");

    cx.simulate_keystrokes(", v");
    cx.run_until_parked();
    unchanged(&view, cx);
    assert_eq!(
        message(&view, cx).as_deref(),
        Some("fe:vim.visual_exit doesn't apply here")
    );

    for (keys, diff) in [
        (", d", HostCommand::DiffGet { rows: 0..1 }),
        (", shift-d", HostCommand::DiffPut { rows: 0..1 }),
    ] {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        unchanged(&view, cx);
        assert_eq!(
            view.update(cx, |view, _| view.take_vim_host_command()),
            Some(diff)
        );
        assert_eq!(
            message(&view, cx).as_deref(),
            Some("a map's keys after a file or window command aren't run")
        );
    }

    // Read-only: an edit is E21, and an ex line runs unless it edits.
    view.update(cx, |view, cx| view.set_read_only(true, cx));
    cx.simulate_keystrokes(", b");
    cx.run_until_parked();
    unchanged(&view, cx);
    assert_eq!(message(&view, cx).as_deref(), Some(E21));
    cx.simulate_keystrokes(", r");
    cx.run_until_parked();
    unchanged(&view, cx);
    assert_eq!(message(&view, cx).as_deref(), Some(E21));
    cx.simulate_keystrokes(", h");
    cx.run_until_parked();
    assert_eq!(caret(&view, cx), Point::new(1, 0));
    assert_eq!(message(&view, cx), None);
}

/// Runs `text` as an ex line typed on the host's command line.
fn submit(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    let submit = Invocation::of("vim.command_line_submit")
        .unwrap()
        .args(serde_json::json!({ "text": text }));
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(crate::commands::RunCommand(submit)), cx)
    });
    cx.run_until_parked();
}

/// A `:normal` whose keys reach a map's `<Cmd>fe:…<CR>` stops there: the
/// keys before it run, the command doesn't, nor anything after it, on the
/// line or the lines after it, or in a map that ran the `:normal`.
#[gpui::test]
fn fe_in_normal_stops_with_a_message(cx: &mut TestAppContext) {
    const REFUSED: &str = "fe: commands don't run inside :normal";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\ntwo\n").unwrap();
    let remaps = vec![
        noremap("<leader>s", "<Cmd>fe:editor.save<CR>x"),
        noremap("<leader>n", "<Cmd>normal x,sx<CR>j"),
        noremap("<leader>m", "<Cmd>normal x,s<CR>"),
    ];
    let (view, cx) = vim_editor(cx, opened(&path), remaps);
    let saves = observed("editor.save", cx);
    let unsaved = |cx: &mut VisualTestContext| {
        assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        assert!(saves.borrow().is_empty());
        assert_eq!(message(&view, cx).as_deref(), Some(REFUSED));
    };

    submit("normal x,sx", cx);
    assert_eq!(text(&view, cx), "ne\ntwo\n");
    unsaved(cx);

    // Over a range: the first line's keys before it, and no line after.
    submit("%normal x,sx", cx);
    assert_eq!(text(&view, cx), "e\ntwo\n");
    unsaved(cx);

    // Run by a map, which stops with it.
    cx.simulate_keystrokes(", n");
    cx.run_until_parked();
    assert_eq!(text(&view, cx), "\ntwo\n");
    assert_eq!(caret(&view, cx).row, 0);
    unsaved(cx);

    // Run inside another `:normal`, over a range: that one stops too.
    view.update(cx, |view, cx| {
        view.model_mut().insert("abc");
        cx.notify();
    });
    submit("%normal ,m", cx);
    assert_eq!(text(&view, cx), "bc\ntwo\n");
    assert_eq!(message(&view, cx).as_deref(), Some(REFUSED));
    assert!(saves.borrow().is_empty());
}

/// A command only a workbench panel's registry has reads as unknown here.
#[gpui::test]
fn workbench_registry_commands_read_as_unknown(cx: &mut TestAppContext) {
    let remaps = vec![
        noremap("<leader>p", "<Cmd>fe:project_panel.focus<CR>j"),
        noremap("<leader>n", "<Cmd>fe:project_panel.select_next<CR>j"),
    ];
    let (view, cx) = vim_editor(cx, text_model("one\ntwo\nthree"), remaps);
    for (keys, id) in [
        (", p", "project_panel.focus"),
        (", n", "project_panel.select_next"),
    ] {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        assert_eq!(
            message(&view, cx),
            Some(format!("E492: Not an editor command: fe:{id}"))
        );
        assert_eq!(caret(&view, cx), Point::new(0, 0));
    }
}

/// A command that applies only while the editor shows a suggestion runs
/// when it does: it's checked and dispatched in the context a key would be
/// matched in, the suggestion's flag stamped live.
#[gpui::test]
fn live_suggestion_is_in_the_context(cx: &mut TestAppContext) {
    let remaps = vec![noremap(
        "<leader>g",
        "a<Cmd>fe:test.suggest<CR><Cmd>fe:ghost_text.accept<CR><Esc>",
    )];
    let (view, cx) = vim_editor(cx, text_model("foo"), remaps);
    let target = view.clone();
    register_app(cx, "test.suggest", move |cx| {
        target.update(cx, |view, cx| {
            let model = view.model();
            let at = model.primary_selection().head();
            let ai = fe_buffer::Origin::Ai {
                agent: "agent".into(),
                session: "session".into(),
            };
            let ghost = fe_editor::ghost_text::GhostText::new(model.snapshot(), at, "bar", ai);
            view.set_ghost_text(Some(ghost), cx);
        })
    });
    let accepts = observed("ghost_text.accept", cx);
    cx.simulate_keystrokes(", g");
    cx.run_until_parked();
    assert_eq!(message(&view, cx), None);
    assert_eq!(text(&view, cx), "fbaroo");
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("normal")
    );
    let shown = accepts.borrow()[0]
        .context
        .frames()
        .iter()
        .any(|frame| frame.name == "editor" && frame.flags.contains("ghost_text"));
    assert!(shown);
}

/// Two windows over files of their own, each with an unsaved edit: the
/// first's editor inside a frame of its own (`holder`), as a pane holds one,
/// focused, its window active; `remaps` set with `,` as the leader.
struct TwoWindows {
    _dir: tempfile::TempDir,
    a: std::path::PathBuf,
    b: std::path::PathBuf,
    first: Entity<EditorView>,
    second: Entity<EditorView>,
    second_window: gpui::AnyWindowHandle,
    first_cx: VisualTestContext,
    second_cx: VisualTestContext,
}

fn two_windows(app: &mut TestAppContext, remaps: Vec<Remap>) -> TwoWindows {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
    fs::write(&a, "one\ntwo\n").unwrap();
    fs::write(&b, "three\nfour\n").unwrap();
    app.update(init);
    let opened_a = a.clone();
    let first_window = app.add_window(move |_, cx| {
        let editor = cx.new(|cx| {
            let model = EditorModel::open(&opened_a).unwrap();
            EditorView::new(model, EditorTheme::default_dark(cx), cx)
        });
        Holder(editor)
    });
    let first = (first_window.read_with(app, |holder, _| holder.0.clone())).unwrap();
    app.update(|cx| set_vim_enabled(cx, true));
    let issues = app.update(|cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    let opened_b = b.clone();
    let window = app.add_window(move |_, cx| {
        let model = EditorModel::open(&opened_b).unwrap();
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    let second = window.root(app).unwrap();
    let second_window = window.into();
    let mut first_cx = VisualTestContext::from_window(first_window.into(), app);
    let mut second_cx = VisualTestContext::from_window(second_window, app);
    for (view, cx) in [(&first, &mut first_cx), (&second, &mut second_cx)] {
        view.update(cx, |view, cx| {
            view.model_mut().insert("new ");
            cx.notify();
        });
    }
    first_cx.update(|window, cx| {
        window.activate_window();
        window.focus(&first.focus_handle(cx), cx);
    });
    first_cx.run_until_parked();
    TwoWindows {
        _dir: dir,
        a,
        b,
        first,
        second,
        second_window,
        first_cx,
        second_cx,
    }
}

impl TwoWindows {
    /// Saved: only the first editor, by `saves`, in its `holder` frame.
    fn only_first_saved(&mut self, saves: &RefCell<Vec<Invocation>>) {
        assert_eq!(fs::read_to_string(&self.a).unwrap(), "new one\ntwo\n");
        assert_eq!(fs::read_to_string(&self.b).unwrap(), "three\nfour\n");
        assert_eq!(caret(&self.second, &mut self.second_cx).row, 0);
        let saves = saves.borrow();
        assert_eq!(saves.len(), 1);
        assert_eq!(saves[0].source, Source::Keyboard);
        let frames = saves[0].context.frames();
        assert!(
            frames.iter().any(|frame| frame.name == "holder"),
            "{frames:?}"
        );
    }
}

/// Keys held in one window that resolve once focus has gone to another run
/// their command on their own editor, in the stack they came in, not on
/// the editor focused now.
#[gpui::test]
fn map_resolved_after_a_blur_runs_where_its_keys_came_in(app: &mut TestAppContext) {
    let remaps = vec![
        noremap("<leader>s", "<Cmd>fe:editor.save<CR>j"),
        noremap("<leader>ss", "x"),
    ];
    let mut t = two_windows(app, remaps);
    let saves = observed("editor.save", &mut t.first_cx);
    t.first_cx.simulate_keystrokes(", s");
    let stack = (t.first_cx).update(|window, _| context_stack(&window.context_stack()));
    let second = t.second.clone();
    t.second_cx.update(|window, cx| {
        window.activate_window();
        window.focus(&second.focus_handle(cx), cx);
    });
    t.first_cx.update(|window, cx| window.blur(cx));
    t.first_cx.update(|window, cx| window.draw(cx).clear(cx));
    t.first_cx.executor().advance_clock(CHORD_TIMEOUT * 2);
    t.first_cx.run_until_parked();

    t.only_first_saved(&saves);
    assert_eq!(caret(&t.first, &mut t.first_cx).row, 1);
    let context = (t.first).update(&mut t.first_cx, |view, cx| view.live_frame(stack, cx));
    let save = Invocation::of("editor.save")
        .unwrap()
        .source(Source::Keyboard)
        .context(context);
    assert_eq!(*saves.borrow(), [save]);
}

/// A command that moves focus to another window, then a recursive map's
/// keys after it starting an ambiguous map there, which resolves at once in
/// an editor that no longer takes keys: its command still runs where the
/// keys came in.
#[gpui::test]
fn map_after_a_focus_change_runs_where_its_keys_came_in(app: &mut TestAppContext) {
    let recursive = Remap {
        recursive: true,
        ..noremap("<leader>f", "<Cmd>fe:test.focus_second<CR>,s")
    };
    let remaps = vec![
        recursive,
        noremap("<leader>s", "<Cmd>fe:editor.save<CR>"),
        noremap("<leader>ss", "x"),
    ];
    let mut t = two_windows(app, remaps);
    let first_window = t.first_cx.update(|window, _| window.window_handle());
    let (window, second) = (t.second_window, t.second.clone());
    // Focus leaves the first window's editor too, as for a panel there; once
    // the dispatch, in the first window, is over, and before the map goes on.
    register_app(&mut t.first_cx, "test.focus_second", move |cx| {
        let second = second.clone();
        cx.defer(move |cx| {
            first_window
                .update(cx, |_, window, cx| window.blur(cx))
                .unwrap();
            let focus = second.focus_handle(cx);
            let focused = window.update(cx, |_, window, cx| {
                window.activate_window();
                window.focus(&focus, cx);
            });
            focused.unwrap();
        });
    });
    let saves = observed("editor.save", &mut t.first_cx);
    t.first_cx.simulate_keystrokes(", f");
    t.first_cx.run_until_parked();
    t.only_first_saved(&saves);
    assert!(!(t.first).read_with(&t.first_cx, |view, _| {
        view.vim_state().unwrap().remap_waiting()
    }));
}

/// A command that hands the host a file or window command ends the map,
/// even once a host that takes it as the editor notifies (the workbench's
/// `follow_vim_keys`) has taken it.
#[gpui::test]
fn diff_command_taken_at_once_still_ends_the_map(cx: &mut TestAppContext) {
    let remaps = vec![
        noremap("<leader>d", "<Cmd>fe:vim.diff_get<CR>x"),
        noremap("<leader>D", "<Cmd>fe:vim.diff_put<CR>x"),
    ];
    let (view, cx) = vim_editor(cx, text_model("one\ntwo"), remaps);
    let taken = Rc::new(RefCell::new(Vec::new()));
    let sink = taken.clone();
    cx.update(|_, cx| {
        cx.observe(&view, move |view, cx| {
            let command = view.update(cx, |view, _| view.take_vim_host_command());
            sink.borrow_mut().extend(command);
        })
        .detach()
    });
    cx.simulate_keystrokes(", d");
    cx.run_until_parked();
    cx.simulate_keystrokes(", shift-d");
    cx.run_until_parked();
    assert_eq!(text(&view, cx), "one\ntwo");
    assert_eq!(
        *taken.borrow(),
        [
            HostCommand::DiffGet { rows: 0..1 },
            HostCommand::DiffPut { rows: 0..1 }
        ]
    );
    assert_eq!(
        message(&view, cx).as_deref(),
        Some("a map's keys after a file or window command aren't run")
    );
}

/// A command the window dispatches later itself, as with nothing focused,
/// still runs before the map's keys after it.
#[gpui::test]
fn command_dispatched_later_runs_before_the_rest(cx: &mut TestAppContext) {
    let remaps = vec![
        noremap("<leader>p", "<Cmd>fe:test.caret<CR>j"),
        noremap("<leader>pp", "x"),
    ];
    let (view, cx) = vim_editor(cx, text_model("one\ntwo"), remaps);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (target, sink) = (view.clone(), seen.clone());
    register_app(cx, "test.caret", move |cx| {
        let model = target.read(cx).model();
        let head = model.primary_selection().head();
        sink.borrow_mut()
            .push(model.text().offset_to_point(head).row);
    });
    cx.simulate_keystrokes(", p");
    cx.update(|window, cx| window.blur(cx));
    cx.executor().advance_clock(CHORD_TIMEOUT * 2);
    cx.run_until_parked();
    assert_eq!(*seen.borrow(), [0]);
    assert_eq!(caret(&view, cx).row, 1);
}

/// A view holding an editor in a frame of its own, `holder`.
struct Holder(Entity<EditorView>);

impl gpui::Render for Holder {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::{InteractiveElement as _, ParentElement as _, Styled as _};
        gpui::div()
            .key_context("holder")
            .size_full()
            .child(self.0.clone())
    }
}

/// A keystroke that comes while a map waits for its command (here the key
/// that breaks a keymap chord, the map's key replayed before it) goes on
/// once the map is over, not between its keys.
#[gpui::test]
fn keys_during_a_command_wait_for_the_map(cx: &mut TestAppContext) {
    let remaps = vec![Remap {
        modes: vec![RemapMode::Normal],
        ..noremap("g", "<Cmd>fe:test.ping<CR>x")
    }];
    let (view, cx) = vim_editor(cx, text_model("one\ntwo\nthree"), remaps);
    register_ping(cx);
    let selects = observed(PING, cx);

    // `g j` breaks `g d`: `g` runs the map, then `j`.
    cx.simulate_keystrokes("g j");
    cx.run_until_parked();
    assert_eq!(selects.borrow().len(), 1);
    assert_eq!(text(&view, cx), "ne\ntwo\nthree");
    assert_eq!(caret(&view, cx), Point::new(1, 0));

    // `g r x` breaks `g r r`: `g` and `r` replayed together, then `x`, so
    // `rx` replaces once the map has deleted.
    cx.simulate_keystrokes("k g r x");
    cx.run_until_parked();
    assert_eq!(selects.borrow().len(), 2);
    assert_eq!(text(&view, cx), "x\ntwo\nthree");
    assert_eq!(caret(&view, cx), Point::new(0, 0));
}
