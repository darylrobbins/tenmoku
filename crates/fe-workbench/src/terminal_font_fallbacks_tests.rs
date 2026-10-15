//! `terminal.font_fallbacks`: a reload changing it restyles the running
//! session.

use std::{
    fs,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_config::{Settings, SettingsEvent, TerminalSettings};
use fe_terminal_view::TerminalView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, tests::open};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn a_reload_restyles_the_running_session_with_the_fallbacks(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    let first = TerminalSettings {
        shell: "/bin/sh".to_owned(),
        shell_args: vec!["-c".to_owned(), "exec cat".to_owned()],
        ..TerminalSettings::default()
    };
    workbench.update(cx, |workbench, cx| {
        workbench.set_terminal_settings(&first, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    cx.simulate_keystrokes("ctrl-`");
    draw(cx);
    let session = |cx: &mut VisualTestContext| -> Entity<TerminalView> {
        workbench
            .read_with(cx, |workbench, _| workbench.terminal().clone())
            .read_with(cx, |panel, _| panel.view().cloned())
            .expect("a session is running")
    };
    let view = session(cx);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !view.read_with(cx, |view, _| view.terminal().is_some()) {
        assert!(Instant::now() < deadline, "timed out waiting for the shell");
        draw(cx);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(view.read_with(cx, |view, _| view.style().font_fallbacks.is_empty()));

    let changed = TerminalSettings {
        font_fallbacks: vec!["Symbols Nerd Font Mono".to_owned()],
        ..first.clone()
    };
    let event = SettingsEvent {
        settings: Arc::new(Settings {
            terminal: changed,
            ..Settings::default()
        }),
        previous: Arc::new(Settings {
            terminal: first,
            ..Settings::default()
        }),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
    let style = view.read_with(cx, |view, _| view.style().clone());
    assert_eq!(style.font_fallbacks, ["Symbols Nerd Font Mono"]);
    assert_eq!(session(cx).entity_id(), view.entity_id());
}
