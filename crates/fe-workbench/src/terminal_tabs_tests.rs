//! Several terminals on the bottom dock: `terminal.new`, `terminal.close`,
//! `terminal.next` and `terminal.previous`, shells exiting, and the tab
//! list shown once there are two.

use std::{
    fs, thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextPredicate, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_terminal_view::TerminalView;
use fe_ui::ActiveTheme as _;
use gpui::{
    Bounds, Entity, EntityId, Focusable as _, Hsla, Modifiers, MouseButton, Pixels, SharedString,
    TestAppContext, VisualTestContext, px,
};

use crate::{Launch, TerminalPanel, Workbench, tests::open};

/// A workbench showing `a.rs` from a temporary project, focused as the app
/// focuses it, whose terminals run `/bin/sh`.
pub(crate) fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    // The pty's threads wake the terminal's event task.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    terminal(&workbench, cx).update(cx, |terminal, _| terminal.set_shell("/bin/sh", &[]));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

pub(crate) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Draws until `done` holds. Panics after 5 s, naming `what`.
pub(crate) fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        draw(cx);
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Runs workbench command `id` as an unbound key or the palette would.
pub(crate) fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

pub(crate) fn toggle(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("ctrl-`");
    draw(cx);
}

pub(crate) fn terminal(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

pub(crate) fn dock_shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    dock.read_with(cx, |dock, _| dock.is_visible())
}

/// The sessions, in start order.
pub(crate) fn sessions(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<Entity<TerminalView>> {
    let terminal = terminal(workbench, cx);
    terminal.read_with(cx, |terminal, _| terminal.views().cloned().collect())
}

pub(crate) fn ids(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<EntityId> {
    let sessions = sessions(workbench, cx);
    sessions.iter().map(Entity::entity_id).collect()
}

pub(crate) fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    terminal(workbench, cx).read_with(cx, |terminal, _| terminal.active())
}

pub(crate) fn focused(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| view.focus_handle(cx).is_focused(window))
}

/// Whether session `ix` is active and has focus.
pub(crate) fn active_and_focused(
    workbench: &Entity<Workbench>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> bool {
    let view = sessions(workbench, cx)[ix].clone();
    active(workbench, cx) == ix && focused(&view, cx)
}

pub(crate) fn editor_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor shows");
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// Waits for `view`'s shell, which starts once the view is drawn.
pub(crate) fn started(view: &Entity<TerminalView>, cx: &mut VisualTestContext) {
    until(cx, "the shell to start", |cx| {
        view.read_with(cx, |view, _| view.terminal().is_some())
    });
}

/// Starts a session with `terminal.new` and waits for its shell.
pub(crate) fn new_session(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<TerminalView> {
    run("terminal.new", cx);
    let view = sessions(workbench, cx).last().unwrap().clone();
    started(&view, cx);
    view
}

/// Shows the terminal with `terminal.toggle`, then starts `more` sessions
/// with `terminal.new`.
pub(crate) fn with_sessions(
    workbench: &Entity<Workbench>,
    more: usize,
    cx: &mut VisualTestContext,
) -> Vec<Entity<TerminalView>> {
    toggle(cx);
    started(&sessions(workbench, cx)[0], cx);
    for _ in 0..more {
        new_session(workbench, cx);
    }
    sessions(workbench, cx)
}

pub(crate) fn type_line(view: &Entity<TerminalView>, line: &str, cx: &mut VisualTestContext) {
    // Enter is a CR, which Windows' console takes and no other end of line.
    let end = if cfg!(windows) { "\r" } else { "\n" };
    let bytes = format!("{line}{end}").into_bytes();
    view.read_with(cx, |view, _| view.terminal().unwrap().input(bytes));
}

/// A command line that sets the session's title to `title`: an OSC 0 from
/// `printf`, or on Windows cmd's `title`.
pub(crate) fn title_command(title: &str) -> String {
    if cfg!(windows) {
        format!("title {title}")
    } else {
        format!(r"printf '\033]0;{title}\007'")
    }
}

/// The labels of the tabs the last frame drew.
pub(crate) fn drawn_tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<SharedString> {
    let terminal = terminal(workbench, cx);
    terminal.read_with(cx, |terminal, _| {
        terminal
            .drawn_tabs()
            .iter()
            .map(|(label, _)| label.clone())
            .collect()
    })
}

/// The backgrounds of the tabs the last frame drew.
pub(crate) fn tab_backgrounds(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<Hsla> {
    let terminal = terminal(workbench, cx);
    terminal.read_with(cx, |terminal, _| {
        terminal.drawn_tabs().iter().map(|(_, bg)| *bg).collect()
    })
}

pub(crate) fn tab_bounds(
    workbench: &Entity<Workbench>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    let terminal = terminal(workbench, cx);
    terminal
        .read_with(cx, |terminal, _| terminal.tab_bounds(ix))
        .expect("the tab was drawn")
}

/// Whether tab `ix` shows whole in the list.
pub(crate) fn tab_shown(
    workbench: &Entity<Workbench>,
    ix: usize,
    cx: &mut VisualTestContext,
) -> bool {
    let tab = tab_bounds(workbench, ix, cx);
    let strip = terminal(workbench, cx).read_with(cx, |terminal, _| terminal.strip_bounds());
    let slack = px(0.5);
    tab.top() >= strip.top() - slack && tab.bottom() <= strip.bottom() + slack
}

#[gpui::test]
fn new_and_cycle(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);

    // With none running and the dock hidden, `terminal.new` shows it with a
    // session, focused.
    run("terminal.new", cx);
    assert!(dock_shown(&workbench, cx));
    assert_eq!(sessions(&workbench, cx).len(), 1);
    assert!(active_and_focused(&workbench, 0, cx));

    // With one session, next and previous do nothing.
    run("terminal.next", cx);
    run("terminal.previous", cx);
    assert!(active_and_focused(&workbench, 0, cx));
    toggle(cx);
    assert!(!dock_shown(&workbench, cx));
    run("terminal.next", cx);
    assert!(!dock_shown(&workbench, cx), "nothing to show");
    assert!(editor_focused(&workbench, cx));

    // New twice from the hidden dock: three sessions, the newest active,
    // focused and shown.
    run("terminal.new", cx);
    assert!(dock_shown(&workbench, cx));
    run("terminal.new", cx);
    assert_eq!(sessions(&workbench, cx).len(), 3);
    assert!(dock_shown(&workbench, cx));
    assert!(active_and_focused(&workbench, 2, cx));
    let count = terminal(&workbench, cx).read_with(cx, |terminal, _| terminal.started);
    assert_eq!(count, 3);

    // Next wraps from the last to the first, previous back again.
    run("terminal.next", cx);
    assert!(active_and_focused(&workbench, 0, cx));
    run("terminal.previous", cx);
    assert!(active_and_focused(&workbench, 2, cx));
    run("terminal.previous", cx);
    assert!(active_and_focused(&workbench, 1, cx));
    run("terminal.next", cx);
    assert!(active_and_focused(&workbench, 2, cx));

    // From the editor, next shows the dock and focuses the session.
    toggle(cx);
    assert!(!dock_shown(&workbench, cx));
    run("terminal.next", cx);
    assert!(dock_shown(&workbench, cx));
    assert!(active_and_focused(&workbench, 0, cx));
}

#[gpui::test]
fn close_activates_a_neighbour(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let all = with_sessions(&workbench, 2, cx);
    let [first, _, third] = [0, 1, 2].map(|ix| all[ix].entity_id());

    // The middle: its right neighbour.
    run("terminal.previous", cx);
    assert!(active_and_focused(&workbench, 1, cx));
    run("terminal.close", cx);
    assert_eq!(ids(&workbench, cx), [first, third]);
    assert!(active_and_focused(&workbench, 1, cx));
    assert!(dock_shown(&workbench, cx));

    // The last: its left neighbour.
    run("terminal.close", cx);
    assert_eq!(ids(&workbench, cx), [first]);
    assert!(active_and_focused(&workbench, 0, cx));
    assert!(dock_shown(&workbench, cx));

    // The only one: the dock hides and the editor has focus, as when the
    // last shell exits.
    run("terminal.close", cx);
    assert!(sessions(&workbench, cx).is_empty());
    assert!(!dock_shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));

    // The next toggle starts a new session, labelled from 1 again.
    toggle(cx);
    assert!(dock_shown(&workbench, cx));
    assert_eq!(sessions(&workbench, cx).len(), 1);
    assert!(active_and_focused(&workbench, 0, cx));
    new_session(&workbench, cx);
    assert_eq!(drawn_tabs(&workbench, cx), ["Terminal 1", "Terminal 2"]);
}

#[gpui::test]
fn exit_drops_only_its_session(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let all = with_sessions(&workbench, 2, cx);
    let [second, third] = [1, 2].map(|ix| all[ix].entity_id());
    assert!(active_and_focused(&workbench, 2, cx));

    // A session in the background: the active one stays, focused.
    type_line(&all[0], "exit", cx);
    until(cx, "the first shell to exit", |cx| {
        ids(&workbench, cx) == [second, third]
    });
    assert!(dock_shown(&workbench, cx));
    assert!(active_and_focused(&workbench, 1, cx));

    // The active, last session: its left neighbour, focused.
    type_line(&all[2], "exit", cx);
    until(cx, "the third shell to exit", |cx| {
        ids(&workbench, cx) == [second]
    });
    assert!(dock_shown(&workbench, cx));
    assert!(active_and_focused(&workbench, 0, cx));

    // The last one: the dock hides and the editor has focus.
    type_line(&all[1], "exit", cx);
    until(cx, "the last shell to exit", |cx| {
        sessions(&workbench, cx).is_empty()
    });
    assert!(!dock_shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));
}

#[gpui::test]
fn tab_strip(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);

    // One session: no list.
    toggle(cx);
    let first = sessions(&workbench, cx)[0].clone();
    started(&first, cx);
    assert!(drawn_tabs(&workbench, cx).is_empty());

    // Two: a tab each, labelled in start order until a shell sets a title.
    let second = new_session(&workbench, cx);
    assert_eq!(drawn_tabs(&workbench, cx), ["Terminal 1", "Terminal 2"]);
    assert!(active_and_focused(&workbench, 1, cx));
    type_line(&second, &title_command("build"), cx);
    until(cx, "the shell to set its title", |cx| {
        drawn_tabs(&workbench, cx) == ["Terminal 1", "build"]
    });

    // A click on a tab activates and focuses its session.
    let tab = tab_bounds(&workbench, 0, cx);
    cx.simulate_click(tab.center(), Modifiers::none());
    draw(cx);
    assert!(active_and_focused(&workbench, 0, cx));
    assert!(focused(&first, cx));

    // The active tab is drawn in the theme's selection colour, which isn't
    // the editor tabs' active colour; the others as inactive tabs.
    let (selected, tab_active, tab_inactive) = cx.update(|_, cx| {
        let colors = &cx.theme().colors;
        (
            colors.list.row_selected,
            colors.tab.active,
            colors.tab.inactive,
        )
    });
    assert_ne!(selected, tab_active);
    assert_ne!(selected, tab_inactive);
    assert_eq!(tab_backgrounds(&workbench, cx), [selected, tab_inactive]);
    run("terminal.next", cx);
    assert_eq!(tab_backgrounds(&workbench, cx), [tab_inactive, selected]);
    run("terminal.next", cx);

    // Toggle hides the dock from the active session, then shows and
    // focuses it again.
    toggle(cx);
    assert!(!dock_shown(&workbench, cx));
    assert!(editor_focused(&workbench, cx));
    toggle(cx);
    assert!(dock_shown(&workbench, cx));
    assert!(active_and_focused(&workbench, 0, cx));

    // Back to one: the list goes.
    run("terminal.next", cx);
    run("terminal.close", cx);
    assert_eq!(sessions(&workbench, cx).len(), 1);
    assert!(drawn_tabs(&workbench, cx).is_empty());
}

#[gpui::test]
fn clicks_follow_sessions(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let all = with_sessions(&workbench, 2, cx);
    let [first, third] = [0, 2].map(|ix| all[ix].entity_id());

    // Pressed on the second's tab, which goes as its shell exits, and
    // released on the third's, drawn there now: no click, as a tab and its
    // handlers name their session, not a place in the list.
    run("terminal.next", cx);
    assert!(active_and_focused(&workbench, 0, cx));
    let second_tab = tab_bounds(&workbench, 1, cx).center();
    cx.simulate_mouse_down(second_tab, MouseButton::Left, Modifiers::none());
    type_line(&all[1], "exit", cx);
    until(cx, "the second shell to exit", |cx| {
        ids(&workbench, cx) == [first, third]
    });
    assert!(tab_bounds(&workbench, 1, cx).contains(&second_tab));
    cx.simulate_mouse_up(second_tab, MouseButton::Left, Modifiers::none());
    draw(cx);
    assert_eq!(active(&workbench, cx), 0, "the first stays active");

    // A tab's close button closes its own session, the active one staying.
    let fourth = new_session(&workbench, cx).entity_id();
    // Its button shows under the pointer.
    let row = cx
        .debug_bounds(format!("terminal-tab-icon-{third}").leak())
        .expect("the third's icon is drawn");
    cx.simulate_mouse_move(row.center(), None, Modifiers::none());
    draw(cx);
    let selector = format!("icon-button:terminal-tab-close-{third}");
    let close = cx
        .debug_bounds(selector.leak())
        .expect("the third's close button is drawn");
    cx.simulate_click(close.center(), Modifiers::none());
    draw(cx);
    assert_eq!(ids(&workbench, cx), [first, fourth]);
    assert_eq!(active(&workbench, cx), 1);
}

#[gpui::test]
fn the_active_tab_scrolls_into_view(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    with_sessions(&workbench, 1, cx);
    // New tabs until the first has scrolled out of the list, the newest
    // shown each time.
    for _ in 0..60 {
        if !tab_shown(&workbench, 0, cx) {
            break;
        }
        run("terminal.new", cx);
        let last = sessions(&workbench, cx).len() - 1;
        assert!(active_and_focused(&workbench, last, cx));
        assert!(tab_shown(&workbench, last, cx), "tab {last} shows");
    }
    assert!(!tab_shown(&workbench, 0, cx), "the tabs overflow the list");
    let last = sessions(&workbench, cx).len() - 1;

    // Cycling past the end brings the first back, and back again the last.
    run("terminal.next", cx);
    assert!(active_and_focused(&workbench, 0, cx));
    assert!(tab_shown(&workbench, 0, cx));
    assert!(!tab_shown(&workbench, last, cx));
    run("terminal.previous", cx);
    assert!(active_and_focused(&workbench, last, cx));
    assert!(tab_shown(&workbench, last, cx));

    // Closing the last shows its left neighbour.
    run("terminal.close", cx);
    assert!(tab_shown(&workbench, last - 1, cx));
}

#[gpui::test]
fn commands_are_in_the_palette(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    for (id, title) in [
        ("terminal.new", "New Terminal"),
        ("terminal.close", "Kill Terminal"),
        ("terminal.next", "Next Terminal"),
        ("terminal.previous", "Previous Terminal"),
    ] {
        let id = fe_command::CommandId::parse(id).unwrap();
        let command = cx.read(|cx| kiln::platform::command_actions::typed_command(&id, cx));
        let command = command.unwrap_or_else(|| panic!("{id} is listed"));
        assert_eq!(command.title, title);
        let workbench = ContextPredicate::identifier(crate::KEY_CONTEXT);
        assert_eq!(command.context, Some(workbench), "{id}");
    }
}
