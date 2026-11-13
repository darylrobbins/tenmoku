//! The title bar in B3 (T1538): 38 px; back, forward, the project widget
//! and the branch widget on the left, side by side with no slash; search
//! and settings on the right; what each opens; and each one's accessible
//! name and tooltip with its key.

use std::{cell::RefCell, fs, path::Path, process::Command, rc::Rc, time::Duration};

use fe_command::{Invocation, Source};
use fe_keymap::{DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, Keymap, KeymapLayer};
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, Pixels, Point, TestAppContext, VisualTestContext,
    point, px,
};

use super::open_root;
use crate::{
    TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET, Workbench,
    open::ConfigDirOverride,
    pane_toolbar::{self, NavButtons},
    settings_editor::SettingsEditor,
    title_bar::{DRAWN, LAST_TOOLTIP},
    workbench::Palette,
};

/// The bar's buttons, left to right, as `(id, debug selector)`.
const LEFT: [(&str, &str); 4] = [
    ("title-back", "icon-button:title-back"),
    ("title-forward", "icon-button:title-forward"),
    ("title-project", "title-project"),
    ("title-branch", "title-branch"),
];
const RIGHT: [(&str, &str); 2] = [
    ("title-search", "icon-button:title-search"),
    ("title-settings", "icon-button:title-settings"),
];

/// Runs `git` in `dir` with the developer's own config shut out.
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

/// A `demo` repository on main with one commit, in a temporary directory,
/// with a `config` directory beside it.
fn demo() -> tempfile::TempDir {
    repo("demo", "main")
}

/// A repository `name` on `branch` with one commit, in a temporary
/// directory, with a `config` directory beside it.
fn repo(name: &str, branch: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join(name);
    fs::create_dir(&work).unwrap();
    fs::create_dir(dir.path().join("config")).unwrap();
    fs::write(work.join("a.rs"), "fn a() {}\n").unwrap();
    git(&work, &["init", "-q"]);
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "init"]);
    if branch != "main" {
        git(&work, &["switch", "-q", "-c", branch]);
    }
    dir
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

/// A workbench on `dir`'s `demo`, its title bar shown, focused, with every
/// command it runs recorded.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    dir: &Path,
) -> (
    Entity<Workbench>,
    &'a mut VisualTestContext,
    Rc<RefCell<Vec<Invocation>>>,
) {
    opened_at(cx, dir, "demo")
}

/// [`opened`], on `dir`'s `name`.
fn opened_at<'a>(
    cx: &'a mut TestAppContext,
    dir: &Path,
    name: &str,
) -> (
    Entity<Workbench>,
    &'a mut VisualTestContext,
    Rc<RefCell<Vec<Invocation>>>,
) {
    let (workbench, cx) = open_root(cx, &dir.join(name));
    let seen = Rc::new(RefCell::new(Vec::new()));
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(Some(dir.join("config"))));
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
        let sink = seen.clone();
        fe_gpui::commands::observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        });
    });
    workbench.update(cx, |workbench, cx| workbench.set_title_bar(true, cx));
    draw(cx);
    (workbench, cx, seen)
}

fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds(selector).expect(selector)
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() <= px(1.)
}

/// The commands run from the mouse since the last call.
fn mouse_runs(seen: &Rc<RefCell<Vec<Invocation>>>) -> Vec<String> {
    let runs = std::mem::take(&mut *seen.borrow_mut());
    (runs.into_iter())
        .filter(|run| run.source == Source::Mouse)
        .map(|run| run.command.as_str().to_owned())
        .collect()
}

fn palette(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Palette> {
    workbench.read_with(cx, |workbench, _| workbench.palette)
}

/// Dismisses whatever palette shows.
fn escape(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("escape");
    draw(cx);
}

fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let at = bounds(selector, cx).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

/// The tooltip shown once the mouse rests on `selector`'s element.
fn hover(selector: &'static str, away: Point<Pixels>, cx: &mut VisualTestContext) -> String {
    cx.simulate_mouse_move(away, None, Modifiers::none());
    draw(cx);
    LAST_TOOLTIP.with_borrow_mut(Option::take);
    let at = bounds(selector, cx).center();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(600));
    draw(cx);
    LAST_TOOLTIP
        .with_borrow_mut(Option::take)
        .unwrap_or_else(|| panic!("no tooltip on {selector}"))
}

#[gpui::test]
fn the_bar_is_38_px_on_top(cx: &mut TestAppContext) {
    let dir = demo();
    let (workbench, cx, _) = opened(cx, dir.path());
    let size = cx.update(|window, _| window.viewport_size());
    let bar = bounds("title-bar", cx);
    assert_eq!(TITLE_BAR_HEIGHT, px(38.));
    assert_eq!(bar.origin, point(px(0.), px(0.)));
    assert_eq!(bar.size.width, size.width);
    assert_eq!(bar.size.height, px(38.));
    let pane = workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).bounds());
    assert_eq!(pane.origin.y, px(38.));
}

#[gpui::test]
fn left_to_right_back_forward_project_branch_then_search_and_settings(cx: &mut TestAppContext) {
    let dir = demo();
    let (workbench, cx, _) = opened(cx, dir.path());
    let width = cx.update(|window, _| window.viewport_size().width);
    let bar = bounds("title-bar", cx);
    let left = LEFT.map(|(_, selector)| bounds(selector, cx));
    let right = RIGHT.map(|(_, selector)| bounds(selector, cx));

    // Every button sits in the bar, centred on it.
    for button in left.iter().chain(&right) {
        assert!(near(button.center().y, bar.center().y), "{button:?}");
    }
    // Back first, clear of the traffic lights; then each after the last.
    assert!(left[0].left() >= TRAFFIC_LIGHT_INSET, "{:?}", left[0]);
    for pair in left.windows(2) {
        assert!(pair[0].right() <= pair[1].left(), "{pair:?}");
    }
    // The project and branch widgets side by side: nothing between them,
    // no slash.
    let gap = left[3].left() - left[2].right();
    assert!(gap >= px(0.) && gap <= px(4.), "{gap:?}");
    let state = workbench.read_with(cx, |workbench, _| workbench.title_bar_state());
    assert_eq!(state.project, "demo");
    assert_eq!(state.branch.as_deref(), Some("main"));
    assert!(!state.project.contains('/') && !state.branch.unwrap().contains('/'));

    // Search, then settings, at the right end.
    assert!(right[0].left() > left[3].right());
    assert!(right[0].right() <= right[1].left());
    assert!(right[1].right() < width && right[1].right() >= width - px(16.));

    // And so the bar draws them, by name.
    let drawn: Vec<_> = DRAWN.with_borrow(|drawn| drawn.iter().map(|(id, _)| *id).collect());
    let ids: Vec<_> = (LEFT.iter().chain(&RIGHT)).map(|(id, _)| *id).collect();
    assert_eq!(drawn, ids);
}

#[gpui::test]
fn no_branch_outside_a_repository(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("plain")).unwrap();
    fs::create_dir(dir.path().join("config")).unwrap();
    let (workbench, cx) = open_root(cx, &dir.path().join("plain"));
    workbench.update(cx, |workbench, cx| workbench.set_title_bar(true, cx));
    draw(cx);
    assert!(cx.debug_bounds("title-project").is_some());
    assert_eq!(cx.debug_bounds("title-branch"), None);
    let state = workbench.read_with(cx, |workbench, _| workbench.title_bar_state());
    assert_eq!((state.project.as_ref(), state.branch), ("plain", None));
}

#[gpui::test]
fn each_opens_what_the_spec_says(cx: &mut TestAppContext) {
    let dir = demo();
    let (workbench, cx, seen) = opened(cx, dir.path());

    // Back and forward are off with no history: a click runs nothing.
    click("icon-button:title-back", cx);
    click("icon-button:title-forward", cx);
    assert_eq!(mouse_runs(&seen), Vec::<String>::new());
    // With somewhere to go, they go.
    for button in ["icon-button:title-back", "icon-button:title-forward"] {
        workbench.update(cx, |workbench, cx| {
            let nav = NavButtons {
                back: true,
                forward: true,
            };
            workbench.set_nav_buttons(nav, cx)
        });
        draw(cx);
        click(button, cx);
    }
    assert_eq!(mouse_runs(&seen), ["nav.back", "nav.forward"]);

    // The project widget opens Open Recent.
    click("title-project", cx);
    assert_eq!(mouse_runs(&seen), ["workbench.open_recent"]);
    assert_eq!(palette(&workbench, cx), Some(Palette::Recent));
    escape(cx);

    // The branch widget, the branch picker.
    click("title-branch", cx);
    assert_eq!(mouse_runs(&seen), ["git.switch_branch"]);
    assert_eq!(palette(&workbench, cx), Some(Palette::Branch));
    escape(cx);

    // Search, Go to File, as its key does.
    click("icon-button:title-search", cx);
    assert_eq!(mouse_runs(&seen), ["quick_open.toggle"]);
    assert_eq!(palette(&workbench, cx), Some(Palette::QuickOpen));
    escape(cx);

    // Settings, the Settings tab.
    click("icon-button:title-settings", cx);
    assert_eq!(mouse_runs(&seen), ["workbench.open_settings_ui"]);
    let settings = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let tab = &pane.tabs()[pane.active().unwrap()];
        tab.item().downcast::<SettingsEditor>().is_some()
    });
    assert!(settings, "the Settings tab is active");

    // No press on them moved or zoomed the window.
    let requests = crate::title_bar::REQUESTS.with_borrow_mut(std::mem::take);
    assert_eq!(requests, []);
}

#[gpui::test]
fn each_has_a_name_and_a_tooltip_with_its_key(cx: &mut TestAppContext) {
    let dir = demo();
    let (workbench, cx, _) = opened(cx, dir.path());
    let away = workbench.read_with(cx, |workbench, cx| {
        workbench.pane().read(cx).bounds().center()
    });

    let names: Vec<_> =
        DRAWN.with_borrow(|drawn| (drawn.iter()).map(|(_, name)| name.to_string()).collect());
    assert_eq!(
        names,
        [
            "Go Back",
            "Go Forward",
            "Project: demo",
            "Branch: main",
            "Go to File",
            "Settings",
        ]
    );

    // The keymap is macOS's; its keys print as this platform prints them.
    let key = |command: &str, cx: &mut VisualTestContext| {
        cx.update(|_, cx| pane_toolbar::shortcut(command, cx))
    };
    let shown = |mac: &str, other: &str| {
        if cfg!(target_os = "macos") {
            mac
        } else {
            other
        }
        .to_owned()
    };
    assert_eq!(
        hover("icon-button:title-search", away, cx),
        shown("Go to File ⌘P", "Go to File Super+P")
    );
    assert_eq!(
        hover("icon-button:title-settings", away, cx),
        shown("Settings ⌘,", "Settings Super+,")
    );
    for (selector, title, command) in [
        ("icon-button:title-back", "Go Back", "nav.back"),
        ("icon-button:title-forward", "Go Forward", "nav.forward"),
        ("title-project", "Open Recent…", "workbench.open_recent"),
    ] {
        let key = key(command, cx).unwrap_or_else(|| panic!("{command} has a key"));
        assert_eq!(hover(selector, away, cx), format!("{title} {key}"));
    }

    // Switch Branch has no key by default; bound, its tooltip names it. The
    // demo's branch has no upstream, which the tooltip says.
    assert_eq!(key("git.switch_branch", cx), None);
    assert_eq!(
        hover("title-branch", away, cx),
        "No upstream: Push publishes this branch"
    );
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    let (user, diagnostics) = KeymapLayer::parse(
        Path::new("keymap.toml"),
        r#"
        [[bindings]]
        keys = "alt-cmd-b"
        command = "git.switch_branch"
        context = "workbench"
        "#,
        None,
    );
    assert_eq!(diagnostics, []);
    let keymap = Keymap::from_layers([default.unwrap(), user.unwrap()]);
    cx.update(|_, cx| fe_gpui::set_user_keymap(cx, keymap));
    assert_eq!(
        hover("title-branch", away, cx),
        shown(
            "No upstream: Push publishes this branch ⌥⌘B",
            "No upstream: Push publishes this branch Alt+Super+B"
        )
    );
}

#[gpui::test]
fn the_breadcrumbs_row_has_no_arrows(cx: &mut TestAppContext) {
    let dir = demo();
    let (workbench, cx, _) = opened(cx, dir.path());
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let file = dir.path().join("demo/a.rs");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(file, window, cx)));
    draw(cx);
    assert!(cx.debug_bounds("pane-toolbar").is_some(), "the row shows");
    for id in ["icon-button:back", "icon-button:forward"] {
        assert_eq!(cx.debug_bounds(id), None, "{id}");
    }
}

#[gpui::test]
fn long_names_give_way_and_both_widgets_still_open(cx: &mut TestAppContext) {
    let name = "a-project-with-a-very-long-folder-name-indeed".repeat(2);
    let branch = "a-branch-with-a-very-long-name-as-well-".repeat(2);
    let dir = repo(&name, &branch);
    let (workbench, cx, seen) = opened_at(cx, dir.path(), &name);
    let state = workbench.read_with(cx, |workbench, _| workbench.title_bar_state());
    assert_eq!(state.branch.as_deref(), Some(branch.as_str()));

    // Wide, each stops at its widest, its name cut short.
    let (project, branch_widget) = (bounds("title-project", cx), bounds("title-branch", cx));
    assert!(project.size.width <= px(240.), "{project:?}");
    assert!(branch_widget.size.width <= px(200.), "{branch_widget:?}");

    // Narrow, both give way and both stay in view, before search.
    for width in [420., 320.] {
        cx.simulate_resize(gpui::size(px(width), px(400.)));
        draw(cx);
        let left = bounds("title-bar-left", cx);
        let project = bounds("title-project", cx);
        let branch_widget = bounds("title-branch", cx);
        let search = bounds("icon-button:title-search", cx);
        assert!(project.size.width >= px(40.), "{width}: {project:?}");
        assert!(
            branch_widget.size.width >= px(40.),
            "{width}: {branch_widget:?}"
        );
        assert!(project.right() <= branch_widget.left(), "{width}");
        assert!(
            branch_widget.right() <= left.right() + px(0.5),
            "{width}: clipped"
        );
        assert!(left.right() <= search.left(), "{width}");
    }

    // And each still opens its picker.
    click("title-project", cx);
    assert_eq!(mouse_runs(&seen), ["workbench.open_recent"]);
    assert_eq!(palette(&workbench, cx), Some(Palette::Recent));
    escape(cx);
    click("title-branch", cx);
    assert_eq!(mouse_runs(&seen), ["git.switch_branch"]);
    assert_eq!(palette(&workbench, cx), Some(Palette::Branch));
}
