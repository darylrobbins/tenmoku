//! The unified title bar: opt-in, the root's name in the project widget,
//! the window dragged and zoomed from its empty space.

use std::path::Path;

use gpui::{
    Bounds, Entity, Modifiers, MouseButton, MouseDownEvent, Pixels, Point, Size, TestAppContext,
    VisualTestContext, point, px,
};

use fe_editor::lane::LaneEvent;
use kiln::{
    core::theme::ThemeData,
    ui::{ActiveTheme as _, Theme, ThemeMode, set_themes},
};
use kiln_shell::StatusSide;

use super::open_root;
use crate::{
    DockPanel, TITLE_BAR_HEIGHT, TRAFFIC_LIGHT_INSET, Workbench,
    lanes::{Tally, chat_lane},
    status_bar::StatusItem,
    title_bar::{CHIP_COLOR, DRAWN, REQUESTS, Request},
};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A temporary directory with a `demo` project in it, and a file in that.
fn demo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("demo")).unwrap();
    std::fs::write(dir.path().join("demo/a.rs"), "fn a() {}\n").unwrap();
    dir
}

fn opened<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    title_bar: bool,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, root);
    let file = root.join("a.rs");
    std::fs::write(&file, "fn a() {}\n").unwrap();
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(file, window, cx)));
    if title_bar {
        workbench.update(cx, |workbench, cx| workbench.set_title_bar(true, cx));
    }
    draw(cx);
    (workbench, cx)
}

fn window(cx: &mut VisualTestContext) -> Size<Pixels> {
    cx.update(|window, _| window.viewport_size())
}

fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds(selector).expect(selector)
}

fn pane_top(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Pixels {
    workbench.read_with(cx, |workbench, cx| {
        workbench.pane().read(cx).bounds().origin.y
    })
}

fn name(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    workbench.read_with(cx, |workbench, _| workbench.title_bar_name().to_string())
}

fn near(a: Pixels, b: Pixels) -> bool {
    (a - b).abs() <= px(1.)
}

fn mouse_down(at: Point<Pixels>, click_count: usize, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
        first_mouse: false,
    });
}

fn requests() -> Vec<Request> {
    REQUESTS.with_borrow_mut(std::mem::take)
}

#[gpui::test]
fn off_by_default(cx: &mut TestAppContext) {
    let dir = demo();
    let (workbench, cx) = opened(cx, &dir.path().join("demo"), false);
    assert_eq!(cx.debug_bounds("title-bar"), None);
    assert_eq!(cx.debug_bounds("title-bar-label"), None);
    assert_eq!(cx.debug_bounds("title-project"), None);
    assert_eq!(pane_top(&workbench, cx), px(0.));
}

#[gpui::test]
fn shows_the_root_name(cx: &mut TestAppContext) {
    let dir = demo();
    let (workbench, cx) = opened(cx, &dir.path().join("demo"), true);
    let size = window(cx);
    let bar = bounds("title-bar", cx);
    assert_eq!((bar.origin.x, bar.origin.y), (px(0.), px(0.)));
    assert_eq!(bar.size.width, size.width);
    assert_eq!(bar.size.height, TITLE_BAR_HEIGHT);
    assert_eq!(TITLE_BAR_HEIGHT, px(38.));
    assert_eq!(name(&workbench, cx), "demo");
    // The project widget reads it, after the traffic lights, in place of a
    // centred title.
    assert_eq!(cx.debug_bounds("title-bar-label"), None);
    let project = bounds("title-project", cx);
    assert!(project.left() > TRAFFIC_LIGHT_INSET, "{project:?}");
    assert!(near(project.center().y, bar.center().y), "{project:?}");
    assert_eq!(pane_top(&workbench, cx), TITLE_BAR_HEIGHT);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let (rail, column) = dock.read_with(cx, |dock, _| {
        (dock.rail_bounds().unwrap(), dock.column_bounds().unwrap())
    });
    assert_eq!(rail.origin.y, TITLE_BAR_HEIGHT);
    assert_eq!(column.origin.y, TITLE_BAR_HEIGHT);
    assert_eq!(bounds("status-bar", cx).bottom(), size.height);

    // Another root retitles it.
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(other, window, cx))
    });
    draw(cx);
    assert_eq!(name(&workbench, cx), "other");

    // A long name stays out of the traffic lights and clear of search and
    // settings, in a narrow window.
    let long = dir.path().join("x".repeat(100));
    std::fs::create_dir(&long).unwrap();
    cx.simulate_resize(gpui::size(px(300.), px(400.)));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(long, window, cx))
    });
    draw(cx);
    let left = bounds("title-bar-left", cx);
    assert!(left.left() >= TRAFFIC_LIGHT_INSET - px(1.), "{left:?}");
    let search = bounds("icon-button:title-search", cx);
    assert!(left.right() <= search.left(), "{left:?} {search:?}");
    assert!(
        bounds("icon-button:title-settings", cx).right() <= px(300.),
        "settings still shows"
    );
}

#[gpui::test]
fn no_root_reads_tenmoku(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let (workbench, cx) = cx.add_window_view(|window, cx| {
        let theme = fe_gpui::EditorTheme::default_dark(cx);
        Workbench::new(
            crate::Launch {
                root: None,
                file: None,
            },
            theme,
            fe_gpui::EditorSettings::default(),
            window,
            cx,
        )
    });
    workbench.update(cx, |workbench, cx| workbench.set_title_bar(true, cx));
    draw(cx);
    assert_eq!(name(&workbench, cx), "Tenmoku");
}

#[gpui::test]
fn drag_and_double_click(cx: &mut TestAppContext) {
    let dir = demo();
    let (workbench, cx) = opened(cx, &dir.path().join("demo"), true);
    requests();
    let bar = bounds("title-bar", cx);
    let left = bounds("title-bar-left", cx);
    let right = bounds("title-bar-right", cx);
    // Between the left and right items.
    let empty = point((left.right() + right.left()) / 2., bar.center().y);
    assert!(left.right() + px(20.) < right.left());

    mouse_down(empty, 1, cx);
    assert_eq!(requests(), [Request::Move]);
    mouse_down(empty, 2, cx);
    assert_eq!(requests(), [Request::DoubleClick]);
    // Left of the items, by the traffic lights.
    mouse_down(point(px(10.), bar.center().y), 1, cx);
    assert_eq!(requests(), [Request::Move]);

    // The items are buttons: a press on one doesn't move the window.
    for selector in [
        "icon-button:title-back",
        "title-project",
        "icon-button:title-settings",
    ] {
        mouse_down(bounds(selector, cx).center(), 1, cx);
        mouse_down(bounds(selector, cx).center(), 2, cx);
        assert_eq!(requests(), [], "{selector}");
    }

    let tab = workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tab_bounds(0));
    mouse_down(tab.expect("a tab").center(), 1, cx);
    let pane = workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).bounds());
    mouse_down(pane.center(), 1, cx);
    mouse_down(pane.center(), 2, cx);
    assert_eq!(requests(), []);
}

/// Lane 1, the chat's, starts or ends a turn.
fn turn(workbench: &Entity<Workbench>, event: LaneEvent, cx: &mut VisualTestContext) {
    workbench.update(cx, |workbench, cx| {
        workbench.lanes.update(cx, |store, cx| {
            store.turn(chat_lane(), event, Tally::default());
            cx.notify();
        });
    });
    draw(cx);
}

/// The chip's text, if the last bar drew one.
fn chip() -> Option<String> {
    DRAWN.with_borrow(|drawn| {
        let (_, text) = drawn.iter().find(|(id, _)| *id == "title-agents")?;
        Some(text.to_string())
    })
}

/// The agents status item's text, if the bar drew one on the left.
fn status_item(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<(usize, String)> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let left = bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Left));
    let at = left.iter().position(|(id, _)| id == StatusItem::AGENTS)?;
    Some((at, left[at].1.to_string()))
}

#[gpui::test]
fn the_agents_chip_counts_working_lanes_after_the_branch_and_shows_the_panel(
    cx: &mut TestAppContext,
) {
    let dir = demo();
    let work = dir.path().join("demo");
    for args in [
        &["init", "-q"][..],
        &["add", "."],
        &["commit", "-q", "-m", "i"],
    ] {
        let output = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(&work)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
    }
    let (workbench, cx) = opened(cx, &work, true);
    // Hidden at zero.
    assert_eq!(chip(), None);
    assert!(cx.debug_bounds("title-agents").is_none());

    turn(&workbench, LaneEvent::TurnStarted, cx);
    assert_eq!(chip().as_deref(), Some("1 agent working"));
    let drawn = bounds("title-agents", cx);
    assert!(drawn.size.width > px(0.));
    // After the project widget, which the branch widget follows in a
    // repository.
    assert!(bounds("title-project", cx).right() <= drawn.left());
    let branch = bounds("title-branch", cx);
    assert!(branch.right() <= drawn.left(), "{branch:?} then {drawn:?}");

    // A second Working lane.
    workbench.update(cx, |workbench, cx| {
        workbench.lanes.update(cx, |store, cx| {
            two(store);
            cx.notify();
        });
    });
    draw(cx);
    assert_eq!(chip().as_deref(), Some("2 agents working"));

    // Where the platform draws the title bar, the status item is the
    // chip's: not here.
    assert_eq!(status_item(&workbench, cx), None);

    // A click shows the Agents panel.
    let at = bounds("title-agents", cx).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    let active = workbench.read_with(cx, |workbench, cx| workbench.dock().read(cx).active());
    assert_eq!(active, DockPanel::Agents);

    // Back to zero: hidden.
    turn(&workbench, LaneEvent::TurnEnded, cx);
    workbench.update(cx, |workbench, cx| {
        workbench.lanes.update(cx, |store, cx| {
            *store = crate::lanes::LaneStore::new();
            cx.notify();
        });
    });
    draw(cx);
    assert_eq!(chip(), None);
}

/// Lane 1's turn running and lane 2, restored, too: two Working.
fn two(store: &mut crate::lanes::LaneStore) {
    *store = crate::lanes::LaneStore::restore(vec![working(2)], Some(3));
    let lane = |n| fe_editor::lane::LaneId::from_number(n).unwrap();
    for id in [chat_lane(), lane(2)] {
        store.turn(id, LaneEvent::TurnStarted, Tally::default());
    }
}

fn working(number: u32) -> fe_editor::lane::Lane {
    use fe_editor::lane::{Backing, Lane, LaneId, LaneKind, LaneState};
    Lane {
        id: LaneId::from_number(number).unwrap(),
        name: format!("agent {number}").into(),
        agent: "acp.claude-agent".into(),
        sessions: vec![format!("s{number}").into()],
        kind: LaneKind::External { host: "acp".into() },
        backing: Backing::Shared,
        state: LaneState::Working,
        started: std::time::SystemTime::UNIX_EPOCH,
        revision: None,
        pending: 0,
    }
}

#[gpui::test]
fn the_agents_status_item_stands_in_where_the_platform_draws_the_title_bar(
    cx: &mut TestAppContext,
) {
    let dir = demo();
    let (workbench, cx) = opened(cx, &dir.path().join("demo"), false);
    assert_eq!(status_item(&workbench, cx), None);

    turn(&workbench, LaneEvent::TurnStarted, cx);
    let (at, text) = status_item(&workbench, cx).expect("shown");
    assert_eq!(text, "1 agent working");
    // On the left, after the vim mode and the operation's place: never
    // before the item's own left items.
    let left = workbench.read_with(cx, |workbench, cx| {
        workbench.status_bar().read(cx).drawn(StatusSide::Left)
    });
    assert_eq!(
        left.last().map(|(id, _)| id.as_ref()),
        Some(StatusItem::AGENTS)
    );
    assert_eq!(at, left.len() - 1);
    assert_eq!(chip(), None, "no chip: the platform draws the title bar");

    // After the operation in progress, not before it.
    let operations = workbench.read_with(cx, |workbench, _| workbench.operations().clone());
    let id = operations.update(cx, |operations, cx| operations.begin("Indexing", cx));
    draw(cx);
    let operation = bounds("status-operation", cx);
    let agents = bounds("status-item:agents", cx);
    assert!(
        operation.right() <= agents.left(),
        "{operation:?} then {agents:?}"
    );
    operations.update(cx, |operations, cx| operations.end(&id, cx));
    draw(cx);

    workbench.update(cx, |workbench, cx| {
        workbench.lanes.update(cx, |store, cx| {
            two(store);
            cx.notify();
        });
    });
    draw(cx);
    assert_eq!(status_item(&workbench, cx).unwrap().1, "2 agents working");

    turn(&workbench, LaneEvent::TurnEnded, cx);
    workbench.update(cx, |workbench, cx| {
        workbench.lanes.update(cx, |store, cx| {
            *store = crate::lanes::LaneStore::new();
            cx.notify();
        });
    });
    draw(cx);
    assert_eq!(status_item(&workbench, cx), None);
}

#[gpui::test]
fn the_agents_chip_takes_agent_base_not_the_accent_in_both_themes(cx: &mut TestAppContext) {
    let dir = demo();
    let (workbench, cx) = opened(cx, &dir.path().join("demo"), true);
    let theme = |title: &str| {
        let theme = fe_config::themes::find(title).expect("bundled");
        Theme::new(ThemeData::from_file(&theme.file()))
    };
    let (yohen, hakuji) = (theme("Yohen"), theme("Hakuji"));
    turn(&workbench, LaneEvent::TurnStarted, cx);
    let mut seen = Vec::new();
    for mode in [ThemeMode::Dark, ThemeMode::Light] {
        cx.update(|_, cx| set_themes(cx, mode, yohen.clone(), hakuji.clone()));
        CHIP_COLOR.set(None);
        draw(cx);
        // Redraw the bar under the new theme.
        workbench.update(cx, |_, cx| cx.notify());
        draw(cx);
        let (agent, accent) = cx.update(|_, cx| {
            let c = &cx.theme().colors;
            (c.agent.base, c.accent.base)
        });
        let drawn = CHIP_COLOR.get().expect("the chip drew");
        assert_eq!(drawn, agent, "{mode:?}: the chip is agent.base");
        assert_ne!(drawn, accent, "{mode:?}: never the accent");
        seen.push(drawn);
    }
    assert_ne!(seen[0], seen[1], "each theme's own agent colour");
}

#[gpui::test]
fn the_agents_status_item_takes_agent_base_and_the_robot_head_not_the_accent_in_both_themes(
    cx: &mut TestAppContext,
) {
    let dir = demo();
    let (workbench, cx) = opened(cx, &dir.path().join("demo"), false);
    let theme = |title: &str| {
        let theme = fe_config::themes::find(title).expect("bundled");
        Theme::new(ThemeData::from_file(&theme.file()))
    };
    let (yohen, hakuji) = (theme("Yohen"), theme("Hakuji"));
    turn(&workbench, LaneEvent::TurnStarted, cx);
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    let mut seen = Vec::new();
    for mode in [ThemeMode::Dark, ThemeMode::Light] {
        cx.update(|_, cx| set_themes(cx, mode, yohen.clone(), hakuji.clone()));
        draw(cx);
        let (agent, soft, accent) = cx.update(|_, cx| {
            let c = &cx.theme().colors;
            (c.agent.base, c.agent.soft, c.accent.base)
        });
        let items = cx.update(|_, cx| bar.read(cx).items(cx));
        let item = items
            .iter()
            .find(|item| item.id == StatusItem::AGENTS)
            .expect("shown");
        let fill = item.fill.expect("a pill in the agent's colours");
        assert_eq!(fill.foreground, agent, "{mode:?}: agent.base");
        assert_eq!(fill.background, soft, "{mode:?}: agent.soft");
        assert_ne!(fill.foreground, accent, "{mode:?}: never the accent");
        assert_ne!(fill.background, accent, "{mode:?}");
        assert_eq!(
            item.icon,
            Some(fe_ui::Icon::Bot),
            "{mode:?}: the robot head"
        );
        assert!(
            cx.debug_bounds("status-icon:agents").is_some(),
            "{mode:?}: drawn"
        );
        seen.push(fill.foreground);
    }
    assert_ne!(seen[0], seen[1], "each theme's own agent colour");
}

#[gpui::test]
fn outside_a_repository_the_chip_has_no_branch_to_follow_and_the_status_item_stays_away(
    cx: &mut TestAppContext,
) {
    // The counterexample to the ordering test: no branch widget, so the
    // chip follows the project widget alone, and the title bar's chip
    // means no status item.
    let dir = demo();
    let (workbench, cx) = opened(cx, &dir.path().join("demo"), true);
    turn(&workbench, LaneEvent::TurnStarted, cx);
    assert!(cx.debug_bounds("title-branch").is_none());
    let project = bounds("title-project", cx);
    let chip = bounds("title-agents", cx);
    assert!(project.right() <= chip.left());
    assert_eq!(status_item(&workbench, cx), None);
    // Hidden at zero with nothing Working: counts hidden in both places.
    workbench.update(cx, |workbench, cx| {
        workbench.lanes.update(cx, |store, cx| {
            *store = crate::lanes::LaneStore::new();
            cx.notify();
        });
    });
    draw(cx);
    assert!(cx.debug_bounds("title-agents").is_none());
}
