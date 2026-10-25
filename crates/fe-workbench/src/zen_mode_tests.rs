//! Zen Mode: `workbench.toggle_zen_mode` (`cmd-k z`) hides the docks, the
//! rail and, as `[workbench.zen_mode]` says, the status bar and the tab
//! strips, centering the panes; toggling it again puts the chrome back as
//! it was. Escape keeps its own keys in it, and a second one leaves it.

use std::{fs, sync::Arc};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{Settings, SettingsEvent, settings::ZenModeSettings};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::PaneView as _;

use crate::{Launch, Pane, Workbench, tests::open};

/// A workbench on a folder with `a.rs` open, focused, and the terminal's
/// shell `/bin/sh`.
pub(super) fn opened(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    workbench.update(cx, |workbench, cx| {
        let terminal = workbench.terminal().clone();
        terminal.update(cx, |terminal, _| terminal.set_shell("/bin/sh", &[]));
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    // Keeps the temporary directory for the test's length.
    std::mem::forget(dir);
    (workbench, cx)
}

/// Gives the window `settings` as the host does
/// ([`Workbench::reload_settings`]), against the defaults.
fn reload(workbench: &Entity<Workbench>, settings: Settings, cx: &mut VisualTestContext) {
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
}

fn zen_settings(zen_mode: ZenModeSettings) -> Settings {
    let mut settings = Settings::default();
    settings.workbench.zen_mode = zen_mode;
    settings
}

pub(super) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

pub(super) fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

/// What of the chrome shows, as drawn: the sidebar, the terminal's dock,
/// the rail, the status bar and the active pane's tab strip.
#[derive(Debug, PartialEq)]
struct Shown {
    sidebar: bool,
    terminal: bool,
    rail: bool,
    status_bar: bool,
    tabs: bool,
}

fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Shown {
    workbench.read_with(cx, |workbench, cx| {
        let dock = workbench.dock().read(cx);
        let tabs = workbench.pane().read(cx).tab_bar_bounds();
        Shown {
            sidebar: dock.is_visible(),
            terminal: workbench.terminal_dock().read(cx).is_visible(),
            rail: dock.rail_bounds().is_some(),
            status_bar: workbench.shell().read(cx).status_bar_visible(),
            tabs: tabs.size.width > gpui::px(0.),
        }
    })
}

const ALL: Shown = Shown {
    sidebar: true,
    terminal: true,
    rail: true,
    status_bar: true,
    tabs: true,
};

const ZEN: Shown = Shown {
    sidebar: false,
    terminal: false,
    rail: false,
    status_bar: false,
    tabs: false,
};

pub(super) fn zen(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, _| workbench.zen_mode())
}

pub(super) fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// `pane`'s left and right margins in the window, as drawn.
fn margins(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> (f32, f32) {
    let bounds = pane.read_with(cx, |pane, _| pane.pane().bounds());
    let width = cx.update(|window, _| window.viewport_size().width);
    let left = f32::from(bounds.left());
    (left, f32::from(width - bounds.right()))
}

#[gpui::test]
fn enter_hides_and_leave_restores(cx: &mut TestAppContext) {
    let (workbench, cx) = opened(cx);
    // The terminal shows beside the sidebar, focus back in the editor.
    keys("ctrl-`", cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(shown(&workbench, cx), ALL);

    keys("cmd-k z", cx);
    assert!(zen(&workbench, cx));
    assert_eq!(shown(&workbench, cx), ZEN);
    // Quitting now would save the sidebar as it was.
    let entry = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(entry["dock"]["visible"], true, "{entry:#}");
    keys("cmd-k z", cx);
    assert!(!zen(&workbench, cx));
    assert_eq!(shown(&workbench, cx), ALL);

    // A sidebar hidden before stays hidden; what's shown meanwhile stays.
    keys("cmd-b", cx);
    let before = Shown {
        sidebar: false,
        ..ALL
    };
    assert_eq!(shown(&workbench, cx), before);
    keys("cmd-k z", cx);
    assert_eq!(shown(&workbench, cx), ZEN);
    keys("cmd-k z", cx);
    assert_eq!(shown(&workbench, cx), before);

    keys("cmd-k z", cx);
    keys("cmd-b", cx);
    assert_eq!(
        shown(&workbench, cx),
        Shown {
            sidebar: true,
            ..ZEN
        }
    );
    keys("cmd-k z", cx);
    assert_eq!(shown(&workbench, cx), ALL);

    // Each shown and hidden again meanwhile stays hidden, and quitting
    // then would save the sidebar hidden.
    keys("cmd-k z", cx);
    keys("cmd-b", cx);
    keys("cmd-b", cx);
    keys("ctrl-`", cx);
    keys("ctrl-`", cx);
    for visible in [true, false] {
        workbench.update(cx, |workbench, cx| {
            workbench.set_status_bar_visible(visible, cx)
        });
    }
    draw(cx);
    assert_eq!(shown(&workbench, cx), ZEN);
    let entry = workbench.read_with(cx, |workbench, cx| workbench.layout_entry(cx));
    assert_eq!(entry["dock"]["visible"], false, "{entry:#}");
    keys("cmd-k z", cx);
    assert_eq!(
        shown(&workbench, cx),
        Shown {
            rail: true,
            tabs: true,
            ..ZEN
        }
    );

    // The status bar turned off in settings meanwhile, though hidden
    // already, stays off.
    workbench.update(cx, |workbench, cx| {
        workbench.set_status_bar_visible(true, cx)
    });
    keys("cmd-k z", cx);
    let mut hidden = Settings::default();
    hidden.status_bar.visible = false;
    reload(&workbench, hidden, cx);
    keys("cmd-k z", cx);
    assert!(!shown(&workbench, cx).status_bar);
}

#[gpui::test]
fn settings_control_what_hides(cx: &mut TestAppContext) {
    let kept = ZenModeSettings {
        hide_tabs: false,
        hide_status_bar: false,
        center_layout: false,
    };
    let (workbench, cx) = opened(cx);
    // The window's own settings, not the app's.
    reload(&workbench, zen_settings(kept), cx);
    keys("cmd-k z", cx);
    assert_eq!(
        shown(&workbench, cx),
        Shown {
            tabs: true,
            status_bar: true,
            ..ZEN
        }
    );
    assert_eq!(margins(&pane(&workbench, cx), cx), (0., 0.));
    keys("cmd-k z", cx);

    // Centered: a fifth of the window's width each side, a split's new
    // pane without its tabs too.
    reload(&workbench, zen_settings(ZenModeSettings::default()), cx);
    keys("cmd-k z", cx);
    assert_eq!(shown(&workbench, cx), ZEN);
    let width = f32::from(cx.update(|window, _| window.viewport_size().width));
    let (left, right) = margins(&pane(&workbench, cx), cx);
    assert!((left - width * 0.2).abs() < 1. && (right - width * 0.2).abs() < 1.);
    keys("cmd-\\", cx);
    let panes = workbench.read_with(cx, |workbench, cx| workbench.panes(cx));
    assert_eq!(panes.len(), 2);
    for pane in &panes {
        assert!(pane.read_with(cx, |pane, _| pane.pane().tab_bar_hidden()));
    }
    keys("cmd-k z", cx);
    assert_eq!(margins(&panes[0], cx).0, 0.);
    for pane in &panes {
        assert!(!pane.read_with(cx, |pane, _| pane.pane().tab_bar_hidden()));
    }
}

#[gpui::test]
fn escape_is_not_delayed_in_zen(cx: &mut TestAppContext) {
    let (workbench, cx) = opened(cx);
    keys("cmd-k z", cx);
    let pane = pane(&workbench, cx);
    let find_open = |cx: &mut VisualTestContext| {
        pane.read_with(cx, |pane, _| pane.find_bar().unwrap().is_open())
    };
    let text = |cx: &mut VisualTestContext| {
        let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
        editor
            .unwrap()
            .read_with(cx, |editor, _| editor.model().text().to_string())
    };

    // A lone escape closes the find bar at once, without the chord timer:
    // the key typed right after it is the editor's.
    keys("cmd-f", cx);
    assert!(find_open(cx));
    keys("escape", cx);
    assert!(!find_open(cx));
    assert!(zen(&workbench, cx));
    keys("cmd-f", cx);
    keys("escape x", cx);
    assert!(!find_open(cx));
    assert_eq!(text(cx), "xfn a() {}\n");

    // It's left by its chord (escape escape is in zen_mode_escape_tests), or
    // its command.
    keys("cmd-k z", cx);
    assert!(!zen(&workbench, cx));
    keys("cmd-k z", cx);
    let run = RunCommand(
        Invocation::of("workbench.toggle_zen_mode")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    assert!(!zen(&workbench, cx));
    assert_eq!(
        shown(&workbench, cx),
        Shown {
            terminal: false,
            ..ALL
        }
    );
}
