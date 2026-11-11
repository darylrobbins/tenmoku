//! The activity rail in B3 (`docs/design/design-system.md`, Workbench
//! anatomy): 42 px wide with 32 px items; Project, Search, Changes and
//! Chat at the top, the panels shown on first use after them, Problems,
//! the Git log and Terminal at the foot; the selection's bar on the edge
//! facing the sidebar; Changes' and Problems' badges; and
//! `workbench.rail`, which moves the rail and the sidebar together.

use std::{fs, path::Path, process::Command, sync::Arc};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{Settings, settings::RailSide};
use fe_editor::diagnostics::{Diagnostic, Severity};
use fe_gpui::{SettingsEvent, commands::RunCommand};
use fe_ui::{BadgeTone, Icon, RAIL_WIDTH, RailButton};
use gpui::{Bounds, Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext, px};
use kiln_shell::{DockPosition, PanelId, dock::RailBadge};

use super::open;
use crate::{Dock, DockPanel, Launch, Workbench, dock::DockOp};

/// A workbench on a temporary project showing `a.rs`, focused as the app
/// focuses it.
fn opened(
    cx: &mut TestAppContext,
    dir: tempfile::TempDir,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let path = dir.path().join("a.rs");
    let text = fs::read_to_string(&path).unwrap_or_else(|_| {
        fs::write(&path, "fn a() {}\n").unwrap();
        "fn a() {}\n".to_owned()
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new(&text).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn dock(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Dock> {
    workbench.read_with(cx, |workbench, _| workbench.dock().clone())
}

fn rail_bounds(dock: &Entity<Dock>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    dock.read_with(cx, |dock, _| dock.rail_bounds().expect("drawn"))
}

fn button(dock: &Entity<Dock>, id: PanelId, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    dock.read_with(cx, |dock, _| {
        dock.state().drawn().rail_button_bounds(id).expect("drawn")
    })
}

fn icons(dock: &Entity<Dock>, cx: &mut VisualTestContext) -> Vec<Icon> {
    dock.read_with(cx, |dock, _| dock.rail_icons())
}

fn badges(dock: &Entity<Dock>, cx: &mut VisualTestContext) -> Vec<(PanelId, RailBadge)> {
    dock.read_with(cx, |dock, _| dock.state().drawn().rail_badges())
}

/// Reloads the window's settings with `workbench.rail` at `side`, from the
/// other side.
fn set_rail(workbench: &Entity<Workbench>, side: RailSide, cx: &mut VisualTestContext) {
    let mut settings = Settings::default();
    settings.workbench.rail = side;
    let mut previous = Settings::default();
    previous.workbench.rail = match side {
        RailSide::Left => RailSide::Right,
        RailSide::Right => RailSide::Left,
    };
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(previous),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
}

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args([
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
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

const PROJECT: PanelId = PanelId("project");
const CHANGES: PanelId = PanelId("scm");
const PROBLEMS: PanelId = PanelId("problems");
const GIT_LOG: PanelId = PanelId("git_log");
const TERMINAL: PanelId = PanelId("terminal");

#[gpui::test]
fn a_42_px_rail_of_32_px_items(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx, tempfile::tempdir().unwrap());
    let dock = dock(&workbench, cx);
    let rail = rail_bounds(&dock, cx);
    assert_eq!(RAIL_WIDTH, px(42.));
    assert_eq!(rail.size.width, RAIL_WIDTH);
    // On the left by default, the sidebar to its right.
    assert_eq!(rail.left(), px(0.), "{rail:?}");
    let column = dock.read_with(cx, |dock, _| dock.column_bounds().unwrap());
    assert_eq!(column.left(), rail.right());
    for id in [PROJECT, CHANGES, PROBLEMS, GIT_LOG, TERMINAL] {
        let button = button(&dock, id, cx);
        assert_eq!(button.size.height, px(32.), "{id:?}");
        assert_eq!(button.size.height, RailButton::HEIGHT, "{id:?}");
        assert_eq!(button.size.width, RAIL_WIDTH, "{id:?}");
        assert_eq!(button.left(), rail.left(), "{id:?}");
    }
}

#[gpui::test]
fn top_and_foot_groups_in_order(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx, tempfile::tempdir().unwrap());
    let dock = dock(&workbench, cx);
    let names: Vec<_> = (DockPanel::ALL.iter()).map(|panel| panel.name()).collect();
    assert_eq!(names, ["Project", "Search", "Changes", "Chat"]);
    let top = [Icon::Files, Icon::Search, Icon::GitBranch, Icon::Bot];
    let foot = [Icon::TriangleAlert, Icon::GitGraph, Icon::FileTerminal];
    assert_eq!(icons(&dock, cx), [&top[..], &foot[..]].concat());

    // The foot is at the rail's bottom, its buttons one after another,
    // well below the top group.
    let rail = rail_bounds(&dock, cx);
    let chat = button(&dock, PanelId("chat"), cx);
    let (problems, log, terminal) = (
        button(&dock, PROBLEMS, cx),
        button(&dock, GIT_LOG, cx),
        button(&dock, TERMINAL, cx),
    );
    assert!(rail.bottom() - terminal.bottom() <= px(4.), "{terminal:?}");
    assert_eq!(log.bottom(), terminal.top());
    assert_eq!(problems.bottom(), log.top());
    assert!(chat.bottom() < problems.top());

    // A panel shown on first use joins the top group, after Chat.
    dock.update_in(cx, |dock, window, cx| {
        dock.run(DockOp::ShowReferences, window, cx)
    });
    draw(cx);
    let joined = [&top[..], &[Icon::FileSymlink], &foot[..]].concat();
    assert_eq!(icons(&dock, cx), joined);

    // The Git log action shows the log in the bottom panel (T1548), the
    // sidebar as it was, the action lit.
    let log = button(&dock, GIT_LOG, cx);
    cx.simulate_click(log.center(), gpui::Modifiers::none());
    draw(cx);
    let ops = dock.read_with(cx, |dock, _| dock.ops().to_vec());
    assert_eq!(ops.last(), Some(&DockOp::ShowGitLog));
    assert_eq!(
        dock.read_with(cx, |dock, _| dock.rail_highlighted()),
        Some(DockPanel::References)
    );
    let lit = dock.read_with(cx, |dock, _| dock.state().drawn().rail_highlighted());
    assert!(lit.contains(&GIT_LOG), "{lit:?}");
}

/// The selection's tinted tab and its bar, as drawn.
fn selection(cx: &mut VisualTestContext) -> (Bounds<Pixels>, Bounds<Pixels>) {
    let tab = cx.debug_bounds("rail-selection").expect("a selection");
    let bar = cx.debug_bounds("rail-bar").expect("a bar");
    (tab, bar)
}

#[gpui::test]
fn the_selection_bar_is_on_the_panel_edge(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx, tempfile::tempdir().unwrap());
    let dock = dock(&workbench, cx);
    let project = button(&dock, PROJECT, cx);
    let (tab, bar) = selection(cx);
    // On the left, the panel faces the rail's right edge: the tab runs to
    // it, inset from the window's edge, and the 2 px bar is on it.
    assert_eq!(bar.size.width, px(2.));
    assert_eq!(bar.right(), project.right());
    assert_eq!((bar.top(), bar.bottom()), (project.top(), project.bottom()));
    assert_eq!(tab.right(), project.right());
    assert!(tab.left() > project.left(), "{tab:?}");
    assert_eq!(tab.size.height, project.size.height);

    // On the right, it flips.
    set_rail(&workbench, RailSide::Right, cx);
    let project = button(&dock, PROJECT, cx);
    let (tab, bar) = selection(cx);
    assert_eq!(bar.size.width, px(2.));
    assert_eq!(bar.left(), project.left());
    assert_eq!(tab.left(), project.left());
    assert!(tab.right() < project.right(), "{tab:?}");

    // Hidden, nothing is selected.
    cx.simulate_keystrokes("cmd-b");
    draw(cx);
    assert_eq!(cx.debug_bounds("rail-bar"), None);
}

#[gpui::test]
fn changes_counts_the_changed_files(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    for file in ["a.rs", "b.rs", "c.rs"] {
        fs::write(root.join(file), format!("{file}\n")).unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("b.rs"), "changed\n").unwrap();
    fs::remove_file(root.join("c.rs")).unwrap();
    fs::write(root.join("d.rs"), "new\n").unwrap();
    let (workbench, cx, dir) = opened(cx, dir);
    let dock = dock(&workbench, cx);
    workbench.update(cx, |workbench, cx| {
        let changes = workbench.dock().read(cx).changes().clone();
        changes.update(cx, crate::GitPanel::refresh);
    });
    draw(cx);
    let expected = RailBadge::count(3, BadgeTone::Modified, "3 changed files");
    assert_eq!(badges(&dock, cx), [(CHANGES, expected)]);

    // Below the icon's bottom-right corner, its fill clear of the glyph,
    // inside the rail.
    let changes = button(&dock, CHANGES, cx);
    let badge = cx.debug_bounds("badge").expect("drawn");
    let icon = cx
        .debug_bounds("rail-icon:dock-rail-2")
        .expect("Changes' icon");
    let ring = fe_ui::BADGE_RING;
    assert!(changes.contains(&badge.origin), "{badge:?} in {changes:?}");
    assert!(
        badge.top() + ring >= icon.bottom(),
        "{badge:?} under {icon:?}"
    );
    assert!(badge.right() > icon.right() && badge.right() < changes.right());

    // Committed, there's no count.
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "all"]);
    workbench.update(cx, |workbench, cx| {
        let changes = workbench.dock().read(cx).changes().clone();
        changes.update(cx, crate::GitPanel::refresh);
    });
    draw(cx);
    assert_eq!(badges(&dock, cx), []);
}

/// Runs workbench command `id` as an unbound key would.
fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn diagnostic(severity: Severity) -> Diagnostic {
    Diagnostic {
        source: Some("test".to_owned()),
        ..Diagnostic::new(severity, "a problem")
    }
}

#[gpui::test]
fn problems_has_a_dot_while_there_are_errors(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx, tempfile::tempdir().unwrap());
    let dock = dock(&workbench, cx);
    let path = dir.path().join("a.rs");
    assert_eq!(badges(&dock, cx), []);

    let set = |severities: &[Severity], cx: &mut VisualTestContext| {
        let diagnostics = (severities.iter())
            .map(|severity| (0..2, diagnostic(*severity)))
            .collect();
        workbench.update(cx, |workbench, cx| {
            workbench.set_diagnostics_for_audit(&path, diagnostics, cx)
        });
        draw(cx);
    };
    set(&[Severity::Error, Severity::Error, Severity::Warning], cx);
    let dot = RailBadge::dot(BadgeTone::Error, "2 errors");
    assert_eq!(badges(&dock, cx), [(PROBLEMS, dot)]);
    let badge = cx.debug_bounds("badge").expect("drawn");
    assert!(button(&dock, PROBLEMS, cx).contains(&badge.origin));

    // Warnings alone: no dot.
    set(&[Severity::Warning], cx);
    assert_eq!(badges(&dock, cx), []);
}

#[gpui::test]
fn rail_setting_applies_live(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx, tempfile::tempdir().unwrap());
    let dock = dock(&workbench, cx);
    let width = cx.update(|window, _| window.viewport_size().width);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let position =
        |cx: &mut VisualTestContext| dock.read_with(cx, |dock, _| dock.state().position());
    assert_eq!(position(cx), DockPosition::Left);

    set_rail(&workbench, RailSide::Right, cx);
    assert_eq!(position(cx), DockPosition::Right);
    let rail = rail_bounds(&dock, cx);
    let column = dock.read_with(cx, |dock, _| dock.column_bounds().unwrap());
    assert_eq!(rail.right(), width, "{rail:?}");
    assert_eq!(column.right(), rail.left(), "the sidebar beside it");
    let editor = pane.read_with(cx, |pane, _| pane.bounds());
    assert_eq!(editor.right(), column.left());

    set_rail(&workbench, RailSide::Left, cx);
    assert_eq!(position(cx), DockPosition::Left);
    let rail = rail_bounds(&dock, cx);
    let column = dock.read_with(cx, |dock, _| dock.column_bounds().unwrap());
    assert_eq!(rail.left(), px(0.), "{rail:?}");
    assert_eq!(column.left(), rail.right());
    let editor = pane.read_with(cx, |pane, _| pane.bounds());
    assert_eq!(editor.left(), column.right());

    // A reload for another key leaves a sidebar moved by command.
    run("dock.move_sidebar_right", cx);
    assert_eq!(position(cx), DockPosition::Right);
    let event = SettingsEvent {
        settings: Arc::new(Settings::default()),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
    assert_eq!(position(cx), DockPosition::Right);
}

#[gpui::test]
fn a_new_language_keeps_the_rail_as_it_was(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx, tempfile::tempdir().unwrap());
    let dock = dock(&workbench, cx);
    let path = dir.path().join("a.rs");
    workbench.update(cx, |workbench, cx| {
        let diagnostics = vec![(0..2, diagnostic(Severity::Error))];
        workbench.set_diagnostics_for_audit(&path, diagnostics, cx);
    });
    dock.update(cx, |dock, cx| {
        dock.set_action_highlighted(crate::dock::TERMINAL_BUTTON, true, cx)
    });
    run("outline.show", cx);
    let rail = |cx: &mut VisualTestContext| {
        dock.read_with(cx, |dock, cx| {
            let drawn = dock.state().drawn();
            let views = dock.explorer().read(cx).stack().read(cx);
            let collapsed: Vec<_> = (views.view_ids().into_iter())
                .map(|view| views.is_collapsed(view))
                .collect();
            (
                drawn.rail_icons(),
                drawn.rail_highlighted(),
                drawn.rail_badges(),
                collapsed,
            )
        })
    };
    let before = rail(cx);
    assert_eq!(before.1, [PROJECT, TERMINAL]);
    assert_eq!(before.2.len(), 1);
    assert_eq!(before.3, [Some(false), Some(false)]);

    // The catalogue's English again, as a language the app ships: the
    // dock takes its text anew and keeps the rest.
    let tag = "en-US".parse().unwrap();
    cx.update(|_, cx| kiln_platform::locale::set_locale(cx, &[tag]));
    draw(cx);
    assert_eq!(rail(cx), before);
}
