//! The Git log in B3 (`docs/design/design-system.md#signals` and
//! `#surfaces`): the graph's lane colours and the avatars' come from the
//! theme's signal roles, never the accent or the agent colour; the card is
//! the floating surface, `surface.raised` with the float shadow, and shows
//! 300 ms after the pointer rests on a row; an agent's commit is told by
//! its trailer.

use std::{fs, path::Path, process::Command, time::Duration};

use fe_gpui::{EditorSettings, EditorTheme};
use fe_ui::{ActiveTheme as _, ROW_HEIGHT, SHADOW_FLOAT};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext, point, px};

use crate::{
    Launch, Workbench,
    workbench::git_commands::git_log::{
        CARD_DELAY, GitLogPanel, GraphColors, float_shadow, is_agent_commit, lane_palette,
    },
};

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository of two commits, the second credited to an agent.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    fs::write(root.join("a.rs"), "one\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "one"]);
    fs::write(root.join("a.rs"), "two\n").unwrap();
    git(root, &["add", "."]);
    let message = "two\n\nCo-Authored-By: Claude <noreply@anthropic.com>";
    git(root, &["commit", "-q", "-m", message]);
    dir
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A window on `root` with the Git Log panel listed and drawn.
fn open(
    cx: &mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, Entity<GitLogPanel>, VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(&mut vcx);
    vcx.update(|window, cx| {
        let run =
            fe_gpui::commands::RunCommand(fe_command::Invocation::of("git.log").unwrap().context(
                fe_command::ContextStack::new().with(fe_command::ContextFrame::new("workbench")),
            ));
        window.dispatch_action(Box::new(run), cx);
    });
    draw(&mut vcx);
    let panel = workbench.read_with(&vcx, |workbench, cx| {
        workbench
            .git_log_panel(cx)
            .expect("the dock has its Git Log")
    });
    (workbench, panel, vcx)
}

#[gpui::test]
fn lane_colours_are_signals_never_the_accent_or_the_agent(cx: &mut TestAppContext) {
    let graph = cx.update(|cx| GraphColors::new(cx));
    cx.update(|cx| {
        let colors = &cx.theme().colors;
        let signals = [
            colors.status.modified,
            colors.status.success,
            colors.status.warning,
            colors.status.error,
        ];
        let lanes: Vec<_> = (0..16).map(|lane| graph.lane(lane)).collect();
        assert!(lanes.len() > 1);
        for lane in &lanes {
            let signals = [
                &signals[..],
                &[
                    colors.vcs.added,
                    colors.vcs.deleted,
                    colors.vcs.conflict,
                    colors.vcs.untracked,
                    colors.status.info,
                ],
            ]
            .concat();
            assert!(signals.contains(lane), "{lane:?} is a signal");
            assert_ne!(*lane, colors.accent.base);
            assert_ne!(*lane, colors.agent.base);
        }
        assert_ne!(graph.lane(0), graph.lane(1), "neighbouring lanes differ");
        // The avatars are from the same list, the same for an email in any
        // case.
        for email in ["a@b.c", "ada@example.com", "t@t"] {
            assert!(lanes.contains(&graph.avatar(email)), "{email}");
        }
        assert_eq!(
            graph.avatar(" Ada@Example.com "),
            graph.avatar("ada@example.com")
        );
        assert_ne!(graph.raised, colors.surface.panel);
        assert_eq!(graph.raised, colors.surface.raised);
    });
}

#[test]
fn an_agents_commit_is_told_by_a_vendor_trailer() {
    let yes = |message| assert!(is_agent_commit(message), "{message}");
    let no = |message| assert!(!is_agent_commit(message), "{message}");
    yes("s\n\nCo-Authored-By: Claude <noreply@anthropic.com>");
    yes("s\n\nco-authored-by: Codex <NOREPLY@openai.com>");
    no("s\n\nCo-authored-by: Ada <ada@example.com>");
    no("s\n\nSigned-off-by: Claude <noreply@anthropic.com>");
    no("Claude <noreply@anthropic.com> wrote this");
    // A person at the vendor is a person; a trailer in the body isn't one.
    no("s\n\nCo-authored-by: Ada <ada@openai.com>");
    no("s\n\nCo-authored-by: Claude <noreply@anthropic.com>\n\nmore text");
    no("Co-authored-by: Claude <noreply@anthropic.com>");
    // A last paragraph of prose with an example in it is no trailer block.
    no("s\n\nIt reads like this:\nCo-authored-by: Claude <noreply@anthropic.com>\nand so on.");
    yes("s\n\nbody\n\nSigned-off-by: A <a@b.c>\nCo-authored-by: Claude <noreply@anthropic.com>");
}

#[test]
fn colliding_roles_leave_no_lane_the_accent_or_the_agent() {
    use gpui::hsla;
    let (accent, agent) = (hsla(0.1, 0.5, 0.5, 1.), hsla(0.6, 0.5, 0.5, 1.));
    let other = hsla(0.3, 0.5, 0.5, 1.);
    let palette = |signals: &[gpui::Hsla]| lane_palette(signals, accent, agent);
    // Only signals, never the reserved two, each once.
    assert_eq!(palette(&[accent, other, agent, other]), [other]);
    // None left: transparent, which is no signal's and no one's colour.
    for signals in [&[accent, agent][..], &[]] {
        let lanes = palette(signals);
        assert_eq!(lanes.len(), 1);
        assert_eq!(lanes[0].a, 0.);
    }
}

#[gpui::test]
fn the_card_is_the_raised_surface_with_the_float_shadow_after_300_ms(cx: &mut TestAppContext) {
    assert_eq!(CARD_DELAY, Duration::from_millis(300));
    let dir = fixture();
    let (_workbench, panel, mut cx) = open(cx, dir.path());
    let cx = &mut cx;
    let probe = panel.read_with(cx, |panel, _| panel.card_probe());
    let list = panel.read_with(cx, |panel, _| panel.list_bounds());
    let row = point(list.left() + px(40.), list.top() + ROW_HEIGHT * 0.5);
    cx.simulate_mouse_move(row, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(299));
    draw(cx);
    assert_eq!(probe.borrow().built, 0, "not before 300 ms");
    cx.executor().advance_clock(Duration::from_millis(2));
    draw(cx);
    draw(cx);
    assert_eq!(probe.borrow().built, 1, "at 300 ms");

    let raised = cx.update(|_, cx| cx.theme().colors.surface.raised);
    let hairline = cx.update(|_, cx| cx.theme().colors.border.default);
    let probe = probe.borrow();
    assert_eq!(probe.background, Some(raised));
    assert_eq!(probe.border, Some(hairline));
    assert_eq!(probe.shadow, vec![float_shadow(raised)]);
    let shadow = &probe.shadow[0];
    assert_eq!(shadow.offset, point(px(0.), SHADOW_FLOAT.y));
    assert_eq!(shadow.blur_radius, SHADOW_FLOAT.blur);
    let graph = cx.update(|_, cx| GraphColors::new(cx));
    let avatar = probe.avatar.expect("an avatar");
    assert!((0..8).any(|lane| graph.lane(lane) == avatar));
}
