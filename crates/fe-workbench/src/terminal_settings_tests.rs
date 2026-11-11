//! The `terminal` settings: the shell a session runs, and a reload restyling
//! the running session.

use std::{
    fs,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_config::{Settings, SettingsEvent, TerminalSettings};
use fe_terminal_view::TerminalView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext, px};

use crate::{
    Launch, TerminalPanel, Workbench,
    tests::{SHELL_END, open, printing_shell},
};

fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.rs");
    fs::write(&path, "fn a() {}\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("fn a() {}\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn until(
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

fn panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

fn session(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalView> {
    panel(workbench, cx)
        .read_with(cx, |panel, _| panel.view().cloned())
        .expect("a session is running")
}

fn screen(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| {
        view.terminal()
            .map(|t| t.snapshot().text())
            .unwrap_or_default()
    })
}

/// Starts a session by showing the terminal, as `terminal.toggle` does, and
/// waits for its shell to start.
fn start(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalView> {
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    cx.simulate_keystrokes("ctrl-`");
    draw(cx);
    let view = session(workbench, cx);
    until(cx, "the shell to start", |cx| {
        view.read_with(cx, |view, _| view.terminal().is_some())
    });
    view
}

/// Reloads from `Settings::default()` to settings with `terminal` set.
fn reload(
    workbench: &Entity<Workbench>,
    previous: &TerminalSettings,
    terminal: TerminalSettings,
    cx: &mut VisualTestContext,
) {
    let event = SettingsEvent {
        settings: Arc::new(Settings {
            terminal,
            ..Settings::default()
        }),
        previous: Arc::new(Settings {
            terminal: previous.clone(),
            ..Settings::default()
        }),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
}

#[gpui::test]
fn a_configured_shell_runs(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    workbench.update(cx, |workbench, cx| {
        workbench.set_terminal_settings(&printing_shell("configured"), cx)
    });
    let view = start(&workbench, cx);
    until(cx, "the configured shell's output", |cx| {
        screen(&view, cx).contains("configured")
    });
}

#[gpui::test]
fn an_empty_shell_is_the_login_shell(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let panel = panel(&workbench, cx);
    panel.read_with(cx, |panel, _| assert_eq!(panel.shell(), None));
    let configured = printing_shell("");
    workbench.update(cx, |workbench, cx| {
        workbench.set_terminal_settings(&configured, cx)
    });
    panel.read_with(cx, |panel, _| {
        assert_eq!(
            panel.shell(),
            Some((configured.shell.clone(), configured.shell_args.clone()))
        )
    });
    workbench.update(cx, |workbench, cx| {
        workbench.set_terminal_settings(&TerminalSettings::default(), cx)
    });
    panel.read_with(cx, |panel, _| assert_eq!(panel.shell(), None));
}

#[gpui::test]
fn a_reload_restyles_the_running_session(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let first = printing_shell("");
    workbench.update(cx, |workbench, cx| {
        workbench.set_terminal_settings(&first, cx)
    });
    let view = start(&workbench, cx);
    let before = view.read_with(cx, |view, _| view.style().clone());

    let sized = TerminalSettings {
        font_size: Some(30.0),
        font_family: Some("Menlo".to_owned()),
        ..first.clone()
    };
    reload(&workbench, &first, sized.clone(), cx);
    let after = view.read_with(cx, |view, _| view.style().clone());
    assert_ne!(before.font_size, after.font_size);
    assert_eq!(after.font_size, px(30.0));
    assert_eq!(after.font_family.as_ref(), "Menlo");
    assert_eq!(session(&workbench, cx).entity_id(), view.entity_id());
}

#[gpui::test]
fn a_reload_changing_the_shell_waits_for_the_next_session(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let first = printing_shell("");
    workbench.update(cx, |workbench, cx| {
        workbench.set_terminal_settings(&first, cx)
    });
    let view = start(&workbench, cx);
    let second = printing_shell("second");
    reload(&workbench, &first, second, cx);
    assert_eq!(session(&workbench, cx).entity_id(), view.entity_id());
    assert!(!screen(&view, cx).contains("second"));

    // The session's shell exits (cat reads EOF), then the next one starts.
    view.read_with(cx, |view, _| {
        view.terminal().unwrap().input(SHELL_END.to_vec())
    });
    until(cx, "the session to end", |cx| {
        panel(&workbench, cx).read_with(cx, |panel, _| panel.view().is_none())
    });
    let next = start(&workbench, cx);
    until(cx, "the new shell's output", |cx| {
        screen(&next, cx).contains("second")
    });
}

/// Unset, the terminal's font is the design's Code small, 12 px on 18 px
/// rows (T349), whatever the editor's size.
#[gpui::test]
fn unset_size_is_code_small(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    workbench.update(cx, |workbench, cx| {
        workbench.set_terminal_settings(&printing_shell(""), cx)
    });
    let view = start(&workbench, cx);
    let style = view.read_with(cx, |view, _| view.style().clone());
    assert_eq!(style.font_size, fe_ui::CODE_SMALL.size);
    assert_eq!(style.font_size, px(12.));
    assert_eq!((style.font_size * style.line_height).round(), px(18.));
    let cell = cx.update(|window, _| fe_terminal_view::cell_size(&style, window));
    assert_eq!(cell.height, fe_ui::CODE_SMALL.line_height);
}

/// T1513's finding: a palette change with no settings reload (a theme
/// file's reload, the system's appearance) restyles the running session
/// and the ones to come: the `[terminal]` table's ANSI colours and
/// overrides, and the editor colours it falls back to.
#[gpui::test]
fn a_palette_change_restyles_the_sessions(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    workbench.update(cx, |workbench, cx| {
        workbench.set_terminal_settings(&printing_shell(""), cx)
    });
    let view = start(&workbench, cx);
    let before = view.read_with(cx, |view, _| view.style().clone());

    let light = fe_config::EditorPalette::fastedit_light();
    let mut palette = light.clone();
    let background = fe_config::Color::rgb(0x123456);
    palette.terminal.background = Some(background);
    palette.terminal.foreground = None;
    palette.terminal.cursor = None;
    cx.update(|_, cx| fe_gpui::set_editor_palette(cx, Arc::new(palette.clone())));
    draw(cx);

    let hsla = |color: fe_config::Color| -> gpui::Hsla { gpui::rgba(color.to_rgba_u32()).into() };
    let ansi = palette.terminal.ansi.map(hsla);
    let check = |style: &fe_terminal_view::TerminalStyle| {
        assert_eq!(style.ansi, ansi);
        assert_eq!(style.background, hsla(background), "the override");
        assert_eq!(
            style.foreground,
            hsla(light.editor.foreground),
            "the editor's"
        );
        assert_eq!(style.cursor, hsla(light.editor.cursor), "the editor's");
    };
    let after = view.read_with(cx, |view, _| view.style().clone());
    assert_ne!(after.ansi, before.ansi);
    check(&after);
    // The font is kept.
    assert_eq!(after.font_size, before.font_size);
    assert_eq!(after.font_family, before.font_family);
    // And a session started after it is drawn in it too.
    panel(&workbench, cx).read_with(cx, |panel, _| check(&panel.style));
}
