// SPDX-License-Identifier: Apache-2.0

//! The workbench's announcements (A11Y-9, the accessibility design's
//! section 5), read off the window's live region, the announcer's `Status`
//! node, with their politeness:
//!
//! - a toast's message and detail once it shows, assertively for an error,
//!   again when an identical one counts it up, and not while it waits;
//!   each toast is an `Alert` named by its message;
//! - the find bar's count, 300 ms after the last change to the query, at
//!   once after Find Next;
//! - Go to Next Problem's diagnostic, at once;
//! - a vim host command's error, assertively, and the vim command line
//!   named by its prefix.

use std::{fs, path::Path, time::Duration};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::diagnostics::{Diagnostic, Severity};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled};
use fe_ui::{Node, Politeness, Role, announce, nodes};
use fe_workbench::{Launch, Workbench};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

const TEXT: &str = "needle one\nneedle two\nneedle three\n";

/// A workbench rooted at a temporary folder, `a.txt` holding [`TEXT`] open
/// and focused, in an active window.
fn opened<'a>(
    dir: &Path,
    cx: &'a mut TestAppContext,
) -> (
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    // See palette_ghost_text.rs: no config directory, no watcher threads.
    for var in ["FASTEDIT_CONFIG_DIR", "XDG_CONFIG_HOME", "HOME"] {
        // SAFETY: no other thread is running yet to read the environment.
        unsafe { std::env::remove_var(var) };
    }
    let path = dir.join("a.txt");
    fs::write(&path, TEXT).unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let launch = Launch {
        root: Some(dir.to_path_buf()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    // Announcements go to the active window.
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).expect("a.txt opens").clone()
    });
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    (workbench, editor, cx)
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }
    cx.run_until_parked();
}

fn drawn(cx: &mut VisualTestContext) -> Vec<Node> {
    draw(cx);
    cx.update(|window, cx| nodes(window, cx))
}

/// What the live region says now, and how urgently.
fn said(cx: &mut VisualTestContext) -> (String, Politeness) {
    let status = (drawn(cx).into_iter().rev())
        .find(|node| node.role == Role::Status)
        .expect("the announcer's Status node");
    let text = status.states.value.map(|value| value.to_string());
    (
        text.unwrap_or_default(),
        status.states.live.unwrap_or_default(),
    )
}

const MARK: &str = "(marker)";

/// Says a marker, so a test can tell that nothing was said after it.
fn mark(cx: &mut VisualTestContext) {
    cx.update(|_, cx| announce(cx, MARK, Politeness::Polite));
    assert_eq!(said(cx).0, MARK);
}

fn polite(text: &str) -> (String, Politeness) {
    (text.to_owned(), Politeness::Polite)
}

fn assertive(text: &str) -> (String, Politeness) {
    (text.to_owned(), Politeness::Assertive)
}

/// Runs `command` as a key would: on the focused element, in the context
/// its key contexts make.
fn run(command: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let context: ContextStack = window
            .context_stack()
            .iter()
            .map(|context| ContextFrame::new(context.primary().map_or("", |entry| &entry.key)))
            .collect();
        let run = RunCommand(Invocation::of(command).unwrap().context(context));
        window.dispatch_action(Box::new(run), cx);
    });
    draw(cx);
}

fn notify(level: log::Level, message: &str, detail: Option<&str>, cx: &mut VisualTestContext) {
    let (message, detail) = (message.to_owned(), detail.map(str::to_owned));
    cx.update(|_, cx| Workbench::notify(cx, level, message, detail));
}

#[gpui::test]
fn a_toast_is_said_as_it_shows_and_is_an_alert(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_workbench, _editor, cx) = opened(dir.path(), cx);

    notify(log::Level::Info, "Saved a.txt", Some("to disk"), cx);
    assert_eq!(said(cx), polite("Saved a.txt\nto disk"));
    let alerts: Vec<Node> = (drawn(cx).into_iter())
        .filter(|node| node.role == Role::Alert)
        .collect();
    assert_eq!(alerts.len(), 1, "{alerts:?}");
    assert_eq!(alerts[0].label, "Saved a.txt");
    assert_eq!(alerts[0].states.description.as_deref(), Some("to disk"));

    notify(log::Level::Error, "Couldn't save b.txt", None, cx);
    assert_eq!(said(cx), assertive("Couldn't save b.txt"));

    // An identical toast counts the first up, and is said again.
    mark(cx);
    notify(log::Level::Info, "Saved a.txt", Some("to disk"), cx);
    assert_eq!(said(cx), polite("Saved a.txt\nto disk"));

    // A fourth waits for room, unsaid, until it shows.
    notify(log::Level::Info, "Indexed", None, cx);
    assert_eq!(said(cx), polite("Indexed"));
    mark(cx);
    notify(log::Level::Warn, "Still waiting", None, cx);
    assert_eq!(said(cx).0, MARK);
    // The info toasts go after their lifetime; the error stays.
    cx.executor().advance_clock(Duration::from_secs(7));
    assert_eq!(said(cx), polite("Still waiting"));
}

#[gpui::test]
fn the_find_count_is_said_after_typing_settles_and_at_once_on_next(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_workbench, _editor, cx) = opened(dir.path(), cx);
    run("find.show", cx);
    mark(cx);
    cx.simulate_input("needle");
    draw(cx);
    assert_eq!(said(cx).0, MARK, "not while the query is typed");
    cx.executor().advance_clock(Duration::from_millis(300));
    assert_eq!(said(cx), polite("1 of 3"));

    run("find.next", cx);
    assert_eq!(said(cx), polite("2 of 3"));
    run("find.previous", cx);
    assert_eq!(said(cx), polite("1 of 3"));

    mark(cx);
    cx.simulate_input("zz");
    cx.executor().advance_clock(Duration::from_millis(200));
    assert_eq!(said(cx).0, MARK, "each key puts it off");
    cx.executor().advance_clock(Duration::from_millis(300));
    assert_eq!(said(cx), polite("No results"));

    // An empty query's wait running out owes nothing to the next query,
    // which waits its own 300 ms.
    // "needlezz", a key at a time.
    cx.simulate_keystrokes(&["backspace"; 8].join(" "));
    cx.executor().advance_clock(Duration::from_millis(300));
    draw(cx);
    mark(cx);
    cx.simulate_input("needle");
    draw(cx);
    assert_eq!(said(cx).0, MARK, "not before its own wait");
    cx.executor().advance_clock(Duration::from_millis(300));
    assert_eq!(said(cx), polite("1 of 3"));
}

#[gpui::test]
fn find_next_with_the_bar_closed_says_the_count(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_workbench, editor, cx) = opened(dir.path(), cx);
    run("find.show", cx);
    cx.simulate_input("needle");
    cx.executor().advance_clock(Duration::from_millis(300));
    assert_eq!(said(cx), polite("1 of 3"));
    run("find.close", cx);
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    mark(cx);
    run("find.next", cx);
    assert_eq!(said(cx), polite("2 of 3"));
    run("find.next", cx);
    assert_eq!(said(cx), polite("3 of 3"));
    run("find.previous", cx);
    assert_eq!(said(cx), polite("2 of 3"));
}

/// Runs `command` as [`run`] does, then `after` before any background
/// count can land.
fn run_then(
    command: &str,
    after: impl FnOnce(&mut gpui::Window, &mut gpui::App),
    cx: &mut VisualTestContext,
) {
    cx.update(|window, cx| {
        let context: ContextStack = window
            .context_stack()
            .iter()
            .map(|context| ContextFrame::new(context.primary().map_or("", |entry| &entry.key)))
            .collect();
        let run = RunCommand(Invocation::of(command).unwrap().context(context));
        window.dispatch_action(Box::new(run), cx);
    });
    // The command has run; the background count waits for the executor.
    cx.update(after);
    draw(cx);
}

#[gpui::test]
fn a_closed_bars_count_is_dropped_once_the_user_has_moved_on(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, editor, cx) = opened(dir.path(), cx);
    run("find.show", cx);
    cx.simulate_input("needle");
    cx.executor().advance_clock(Duration::from_millis(300));
    run("find.close", cx);

    // The caret moves before the count lands.
    mark(cx);
    let moved = editor.clone();
    run_then(
        "find.next",
        move |_, cx| {
            moved.update(cx, |editor, cx| editor.select_and_reveal(0..0, cx));
        },
        cx,
    );
    assert_eq!(said(cx).0, MARK, "after a move");

    // An edit before it lands.
    let edited = editor.clone();
    run_then(
        "find.next",
        move |_, cx| {
            edited.update(cx, |editor, cx| {
                let end = editor.model().text().len();
                editor
                    .model_mut()
                    .buffer_mut()
                    .edit([(end..end, "x")], fe_buffer::Origin::User);
                cx.notify();
            });
        },
        cx,
    );
    assert_eq!(said(cx).0, MARK, "after an edit");

    // Focus leaving the editor, as a tab or window switch takes it.
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    run_then(
        "find.next",
        move |window, cx| window.focus(&panel.focus_handle(cx), cx),
        cx,
    );
    assert_eq!(said(cx).0, MARK, "after focus left");

    // Undisturbed, it's said.
    assert!(!cx.update(|window, cx| editor.focus_handle(cx).contains_focused(window, cx)));
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    run("find.next", cx);
    assert!(said(cx).0.ends_with(" of 3"), "{:?}", said(cx));
}

#[gpui::test]
fn go_to_next_problem_says_the_diagnostic_at_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_workbench, editor, cx) = opened(dir.path(), cx);
    let two = TEXT.find("two").unwrap();
    editor.update(cx, |editor, cx| {
        let version = editor.model().snapshot().version();
        let items = vec![(
            two..two + 3,
            Diagnostic::new(Severity::Error, "not a number"),
        )];
        let id = (1, 1);
        assert!((editor.model_mut()).apply_diagnostics(1u32, id, version, items, Some(id)));
        cx.notify();
    });
    draw(cx);
    mark(cx);
    run("diagnostics.next", cx);
    assert_eq!(said(cx), polite("error: not a number"));
}

#[gpui::test]
fn vim_host_errors_are_assertive_and_the_command_line_is_named(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_workbench, _editor, cx) = opened(dir.path(), cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);

    for (key, name) in [
        (":", "Command"),
        ("/", "Search forward"),
        ("?", "Search backward"),
        (":", "Command"),
    ] {
        cx.simulate_keystrokes(key);
        let inputs: Vec<String> = (drawn(cx).into_iter())
            .filter(|node| matches!(node.role, Role::TextInput | Role::SearchInput))
            .map(|node| node.label.to_string())
            .collect();
        assert!(inputs.contains(&name.to_string()), "{key}: {inputs:?}");
        cx.simulate_keystrokes("escape");
        draw(cx);
    }

    // `:e` with no file is the host's to answer, an error with no code.
    mark(cx);
    cx.simulate_keystrokes(":");
    cx.simulate_input("e");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        said(cx),
        assertive("Not supported yet: reloading a file from disk")
    );
}

/// A plain window with an announcer, to make active instead.
struct Elsewhere;

impl gpui::Render for Elsewhere {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::{ParentElement as _, Styled as _};
        gpui::div()
            .size_full()
            .child(fe_ui::Announcer::mount(window, cx))
    }
}

/// Opens [`Elsewhere`] and makes it the active window.
fn activate_elsewhere(cx: &mut VisualTestContext) -> gpui::AnyWindowHandle {
    let elsewhere = cx.update(|_, cx| {
        use gpui::AppContext as _;
        cx.open_window(Default::default(), |_, cx| cx.new(|_| Elsewhere))
            .unwrap()
    });
    let elsewhere: gpui::AnyWindowHandle = elsewhere.into();
    (elsewhere.update(cx, |_, window, _| window.activate_window())).unwrap();
    cx.run_until_parked();
    elsewhere
}

/// What `window`'s live region says, drawn afresh.
fn said_in(window: gpui::AnyWindowHandle, cx: &mut VisualTestContext) -> Option<String> {
    cx.run_until_parked();
    let nodes = window
        .update(cx, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            nodes(window, cx)
        })
        .unwrap();
    (nodes.into_iter().rev())
        .find(|node| node.role == Role::Status)
        .and_then(|node| node.states.value.map(|value| value.to_string()))
}

#[gpui::test]
fn a_count_owed_when_another_window_becomes_active_is_dropped(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_workbench, _editor, cx) = opened(dir.path(), cx);
    run("find.show", cx);
    mark(cx);
    cx.simulate_input("needle");
    draw(cx);
    // Another window comes forward before the query's wait is up.
    let elsewhere = activate_elsewhere(cx);
    cx.executor().advance_clock(Duration::from_millis(300));
    draw(cx);
    assert_eq!(said_in(elsewhere, cx), None, "not said in the other window");
    assert_eq!(said(cx).0, MARK, "nor in its own");
}

#[gpui::test]
fn a_toast_shown_in_an_inactive_window_is_not_said(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_workbench, _editor, cx) = opened(dir.path(), cx);
    // Three shown, the fourth waiting for room.
    for message in ["One", "Two", "Three"] {
        notify(log::Level::Info, message, None, cx);
    }
    notify(log::Level::Warn, "Still waiting", None, cx);
    mark(cx);
    let elsewhere = activate_elsewhere(cx);
    // The info toasts go, and the waiting one shows in the inactive window.
    cx.executor().advance_clock(Duration::from_secs(7));
    draw(cx);
    assert_eq!(said_in(elsewhere, cx), None, "not said in the other window");
    assert_eq!(said(cx).0, MARK, "nor in its own");
}

/// Types `line` as an ex command in the focused vim editor.
fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

#[gpui::test]
fn a_typed_git_failure_is_assertive(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_workbench, _editor, cx) = opened(dir.path(), cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);

    // The folder is no repository: `answer_git_op` answers with a failure.
    mark(cx);
    ex("Git fetch", cx);
    assert_eq!(said(cx), assertive("Not in a git repository"));
}

#[gpui::test]
fn a_quickfix_failure_is_assertive_and_a_report_polite(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_workbench, _editor, cx) = opened(dir.path(), cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);

    // A report: the list's summary, with no jump.
    mark(cx);
    ex("vimgrep /needle/j **", cx);
    let (text, politeness) = said(cx);
    assert_eq!(politeness, Politeness::Polite, "{text}");
    assert_ne!(text, MARK);

    // A failure with no code: no default build task to run.
    mark(cx);
    ex("make", cx);
    cx.executor().advance_clock(Duration::from_secs(1));
    draw(cx);
    let (text, politeness) = said(cx);
    assert_eq!(politeness, Politeness::Assertive, "{text}");
    assert!(!text.starts_with('E'), "an uncoded failure: {text}");
    assert_ne!(text, MARK);
}
