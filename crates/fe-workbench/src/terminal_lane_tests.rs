//! A lane's terminal tab (agent lanes card 13): silver-blue, `lane 2 ·
//! <command>` when an agent's and read-only, writable when a terminal
//! lane's own.

use fe_config::themes;
use fe_editor::lane::LaneId;
use fe_terminal_view::{TerminalOptions, TerminalView};
use fe_ui::{ActiveTheme as _, IconRef, Theme, ThemeData};
use gpui::{Entity, Focusable as _, Hsla, TestAppContext, VisualTestContext};

use super::terminal_tabs_tests::{draw, opened, sessions, started, terminal, toggle, until};
use crate::{Workbench, tests::posix_sh};

fn lane(number: u32) -> LaneId {
    LaneId::from_number(number).unwrap()
}

/// Opens a session of `lane`, which runs [`posix_sh`] (`/bin/sh`, or Git
/// for Windows' `sh.exe`), and waits for it.
fn add_lane_session(
    workbench: &Entity<Workbench>,
    lane: LaneId,
    read_only: bool,
    command: &str,
    cx: &mut VisualTestContext,
) -> Entity<TerminalView> {
    let options = TerminalOptions {
        // OSC 133;C marks a command running: the tab's spinner. It then
        // answers each line it reads.
        shell: Some((
            posix_sh().to_string_lossy().into_owned(),
            vec![
                "-c".to_owned(),
                r"printf '\033]133;C\007ready\n'; while read l; do echo got:$l; done".to_owned(),
            ],
        )),
        ..TerminalOptions::new(std::env::temp_dir())
    };
    let panel = terminal(workbench, cx);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.add_lane_session(lane, read_only, command.to_owned(), options, window, cx)
        })
    });
    let view = sessions(workbench, cx).last().unwrap().clone();
    started(&view, cx);
    draw(cx);
    view
}

/// What the last frame drew for each tab: label, its text colour if it was
/// given one, and its icon.
fn drawn(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(String, Option<Hsla>, IconRef)> {
    let panel = terminal(workbench, cx);
    panel.read_with(cx, |panel, _| {
        (panel.drawn_tabs().iter())
            .zip(panel.drawn_tab_marks())
            .map(|((label, _), (color, icon))| (label.to_string(), *color, icon.clone()))
            .collect()
    })
}

fn screen(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.terminal().unwrap().snapshot().text())
}

/// Types `line` into `view` and presses enter.
fn type_in(view: &Entity<TerminalView>, line: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
}

fn agent_color(cx: &mut VisualTestContext) -> Hsla {
    cx.update(|_, cx| cx.theme().colors.agent.base)
}

#[gpui::test]
fn an_agents_terminal_is_silver_blue_named_for_its_lane_and_takes_no_typing(
    cx: &mut TestAppContext,
) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    started(&sessions(&workbench, cx)[0], cx);
    let view = add_lane_session(&workbench, lane(2), true, "cargo test", cx);

    let agent = agent_color(cx);
    let tabs = drawn(&workbench, cx);
    assert_eq!(tabs.len(), 2);
    // The user's own shell keeps the theme's tab text and icon.
    assert_eq!(tabs[0].0, "Terminal 1");
    assert_eq!(tabs[0].1, None);
    assert_eq!(tabs[0].2, IconRef::Bundled(fe_ui::Icon::FileTerminal));
    // The agent's: named for its lane and command, its text and icon in the
    // agent's colour.
    let (label, text, icon) = &tabs[1];
    assert_eq!(label, "lane 2 · cargo test");
    assert_eq!(*text, Some(agent));
    let IconRef::Tinted(_, token) = icon else {
        panic!("the icon isn't tinted: {icon:?}");
    };
    let colors = cx.update(|_, cx| cx.theme().colors);
    assert_eq!(colors.get(token).copied(), Some(agent));

    // Its command is running, so the spinner is up, in the agent's colour.
    until(cx, "the spinner", |cx| {
        matches!(
            drawn(&workbench, cx)[1].2,
            IconRef::Tinted(fe_ui::Icon::LoaderCircle, "agent.base")
        )
    });
    let colors = cx.update(|_, cx| cx.theme().colors);
    assert_eq!(colors.get("agent.base").copied(), Some(agent));

    // It takes no typing: the program, which answers each line it reads,
    // hears none, though the output has had time to arrive.
    assert!(view.read_with(cx, |view, _| view.is_read_only()));
    type_in(&view, "hello", cx);
    cx.simulate_keystrokes("a b c");
    until(cx, "ready", |cx| screen(&view, cx).contains("ready"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while std::time::Instant::now() < deadline {
        draw(cx);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let text = screen(&view, cx);
    assert!(!text.contains("got:") && !text.contains("hello"), "{text}");
}

#[gpui::test]
fn a_terminal_lanes_own_tab_is_silver_blue_and_writable(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    let view = add_lane_session(&workbench, lane(3), false, "claude", cx);

    let agent = agent_color(cx);
    until(cx, "the spinner", |cx| {
        matches!(
            drawn(&workbench, cx)[1].2,
            IconRef::Tinted(fe_ui::Icon::LoaderCircle, "agent.base")
        )
    });
    let (label, text, icon) = drawn(&workbench, cx).remove(1);
    // Not the agent's own run, so the shell's name stands.
    assert!(label.starts_with("Terminal"), "{label}");
    assert_eq!(text, Some(agent));
    assert!(matches!(icon, IconRef::Tinted(_, "agent.base")), "{icon:?}");
    assert!(!view.read_with(cx, |view, _| view.is_read_only()));
    // It takes typing: the program answers the line.
    type_in(&view, "hello", cx);
    until(cx, "the program's answer", |cx| {
        screen(&view, cx).contains("got:hello")
    });
}

#[gpui::test]
fn lane_tabs_take_the_agent_token_not_the_accent_in_both_themes(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    toggle(cx);
    add_lane_session(&workbench, lane(2), true, "make", cx);
    for name in [themes::DEFAULT_DARK, themes::DEFAULT_LIGHT] {
        let data = ThemeData::from_file(&themes::find(name).unwrap().file());
        let want: Hsla = gpui::rgba(data.tokens.agent.base.to_rgba_u32()).into();
        cx.update(|window, cx| {
            fe_ui::set_theme(cx, Theme::new(data));
            window.refresh();
        });
        terminal(&workbench, cx).update(cx, |_, cx| cx.notify());
        draw(cx);
        let colors = cx.update(|_, cx| cx.theme().colors);
        assert_ne!(colors.agent.base, colors.accent.base, "{name}");
        let (_, text, icon) = drawn(&workbench, cx).remove(1);
        assert_eq!(text, Some(colors.agent.base), "{name}");
        assert_eq!(text, Some(want), "{name}");
        assert_ne!(text, Some(colors.accent.base), "{name}");
        let IconRef::Tinted(_, token) = icon else {
            panic!("{name}: the icon isn't tinted");
        };
        assert_eq!(colors.get(token).copied(), text, "{name}");
    }
}
