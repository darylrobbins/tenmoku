//! Git sync in the UI (T332): the title bar's branch widget shows how far the
//! branch is from its upstream, `main` and `↑2 ↓1` beside it, its tooltip naming the
//! upstream; the Changes panel's header shows the same, with Pull, Push (or
//! Publish Branch) and Fetch buttons, off while one runs; and both follow a
//! fetch, pull, push or commit. On repositories built with the `git` CLI in
//! temporary directories, each with a local bare `origin`, so no network.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{
    GitPanel, Launch, Workbench,
    git_panel::{SyncHeader, set_box_text},
    title_bar::LAST_TOOLTIP,
    workbench::{
        git_commands::{GitCommand, Routed},
        toast::Level,
    },
};

/// Runs `git` in `dir`, with the developer's own config shut out; its output,
/// trimmed.
fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

struct Fixture {
    _dir: tempfile::TempDir,
    /// What the workbench is rooted at, on main with `a.txt`, tracking
    /// origin's main. Its own config shuts out the developer's where it
    /// matters, as the workbench's git reads that too.
    work: PathBuf,
    /// The bare remote.
    origin: PathBuf,
    /// Another clone, to move the remote with.
    other: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(dir.path()).unwrap();
    git(&base, &["init", "-q", "--bare", "origin.git"]);
    let work = base.join("work");
    fs::create_dir(&work).unwrap();
    git(&work, &["init", "-q"]);
    for (key, value) in [
        ("user.name", "t"),
        ("user.email", "t@t"),
        ("commit.gpgsign", "false"),
        ("push.gpgsign", "false"),
        ("fetch.prune", "false"),
        ("pull.rebase", "false"),
    ] {
        git(&work, &["config", key, value]);
    }
    fs::write(work.join("a.txt"), "one\n").unwrap();
    commit(&work, "init");
    git(&work, &["remote", "add", "origin", "../origin.git"]);
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    git(&base, &["clone", "-q", "origin.git", "other"]);
    Fixture {
        origin: base.join("origin.git"),
        other: base.join("other"),
        work,
        _dir: dir,
    }
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", message]);
}

/// Commits a new file `name` in `dir`.
fn commit_file(dir: &Path, name: &str) {
    fs::write(dir.join(name), format!("{name}\n")).unwrap();
    commit(dir, name);
}

/// Commits `name` from the other clone, brought up to date, and pushes it.
fn move_remote(fixture: &Fixture, name: &str) {
    git(
        &fixture.other,
        &["pull", "-q", "--ff-only", "origin", "main"],
    );
    commit_file(&fixture.other, name);
    git(&fixture.other, &["push", "-q", "origin", "main"]);
}

fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"])
}

/// A workbench rooted at `root` in a headless window, its title bar shown,
/// focused, showing the Changes panel.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (
    Entity<Workbench>,
    Entity<GitPanel>,
    &'a mut VisualTestContext,
) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    workbench.update(cx, |workbench, cx| workbench.set_title_bar(true, cx));
    draw(cx);
    let run = RunCommand(
        Invocation::of("git.show_changes")
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).changes().clone()
    });
    (workbench, panel, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    cx.run_until_parked();
}

/// Reads git's status again, as a watcher batch would have it.
fn refresh(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let git = workbench.read_with(cx, |workbench, _| workbench.git().clone());
    git.update(cx, |git, cx| git.refresh(cx));
    draw(cx);
}

/// What the title bar's branch widget shows.
fn branch_item(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let state = workbench.read_with(cx, |workbench, _| workbench.title_bar_state());
    state.branch.map(|branch| branch.to_string())
}

/// The counts the branch widget shows beside the name, drawn as their own element.
fn branch_sync(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let state = workbench.read_with(cx, |workbench, _| workbench.title_bar_state());
    let tracking = match state.tracking {
        crate::title_bar::Tracking::Upstream(upstream) => Some(upstream),
        _ => None,
    };
    let counts = crate::git::sync_counts(tracking.as_ref());
    assert_eq!(
        counts.is_some(),
        cx.debug_bounds("title-branch-sync").is_some()
    );
    counts
}

/// The tooltip shown once the mouse rests on the branch widget.
fn branch_tooltip(cx: &mut VisualTestContext) -> String {
    cx.simulate_mouse_move(
        gpui::point(gpui::px(600.), gpui::px(400.)),
        None,
        Modifiers::none(),
    );
    draw(cx);
    LAST_TOOLTIP.with_borrow_mut(Option::take);
    let at = cx
        .debug_bounds("title-branch")
        .expect("the branch widget's drawn");
    cx.simulate_mouse_move(at.center(), None, Modifiers::none());
    cx.executor().advance_clock(Duration::from_millis(600));
    draw(cx);
    LAST_TOOLTIP
        .with_borrow_mut(Option::take)
        .expect("a tooltip on the branch widget")
}

fn header(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> Option<SyncHeader> {
    panel.read_with(cx, |panel, cx| panel.sync_header(cx))
}

/// The header's buttons' names, with whether each is on.
fn buttons(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
    let header = header(panel, cx).expect("the sync header shows");
    (header.buttons.into_iter())
        .map(|(_, name, on)| (name, on))
        .collect()
}

/// Clicks the header's button `id` where the last frame drew it.
fn click(id: &str, cx: &mut VisualTestContext) {
    let selector = format!("icon-button:{id}");
    let bounds = cx.debug_bounds(selector.leak()).expect("drawn");
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

/// The shown toasts' levels, messages and whether each offers choices,
/// emptied.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(Level, String, bool)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    let shown = layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let toast = &entry.toast;
                (toast.level, toast.message.clone(), toast.choices.is_some())
            })
            .collect()
    });
    cx.update(|_, cx| layer.update(cx, |layer, cx| layer.dismiss_all(cx)));
    shown
}

/// The git commands routed since the last call.
fn routed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<GitCommand> {
    workbench.update(cx, |workbench, _| {
        (workbench.git_routed.drain(..))
            .filter_map(|routed| match routed {
                Routed::Op(op) => Some(op.command),
                _ => None,
            })
            .collect()
    })
}

#[gpui::test]
fn the_branch_widget_shows_ahead_and_behind_counts(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    let (workbench, panel, cx) = open(cx, work);

    // Level with its upstream: nothing extra, but the tooltip names it.
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    assert_eq!(branch_sync(&workbench, cx), None);
    let tooltip = branch_tooltip(cx);
    assert!(tooltip.contains("origin/main"), "{tooltip}");
    assert!(tooltip.contains("0 to push, 0 to pull"), "{tooltip}");
    assert!(tooltip.contains("as of the last fetch"), "{tooltip}");
    let shown = header(&panel, cx).expect("the sync header shows");
    assert_eq!(shown.upstream.as_deref(), Some("origin/main"));
    assert_eq!(shown.counts, None);

    // Ahead only.
    commit_file(work, "b.txt");
    commit_file(work, "c.txt");
    refresh(&workbench, cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    assert_eq!(branch_sync(&workbench, cx).as_deref(), Some("↑2"));

    // Ahead and behind, as of the last fetch: none until then.
    move_remote(&fixture, "d.txt");
    refresh(&workbench, cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    assert_eq!(branch_sync(&workbench, cx).as_deref(), Some("↑2"));
    git(work, &["fetch", "-q"]);
    refresh(&workbench, cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    assert_eq!(branch_sync(&workbench, cx).as_deref(), Some("↑2 ↓1"));
    let shown = header(&panel, cx).expect("the sync header shows");
    assert_eq!(shown.counts.as_deref(), Some("↑2 ↓1"));
    assert!(cx.debug_bounds("changes-sync-counts").is_some());

    // After the marks.
    fs::write(work.join("a.txt"), "edited\n").unwrap();
    refresh(&workbench, cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main*"));
    assert_eq!(branch_sync(&workbench, cx).as_deref(), Some("↑2 ↓1"));

    // Behind only.
    git(work, &["reset", "-q", "--hard", "origin/main"]);
    refresh(&workbench, cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    git(work, &["reset", "-q", "--hard", "HEAD~1"]);
    refresh(&workbench, cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    assert_eq!(branch_sync(&workbench, cx).as_deref(), Some("↓1"));
}

#[gpui::test]
fn no_upstream_shows_a_publish_affordance_not_counts(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    git(work, &["switch", "-q", "-c", "feat"]);
    commit_file(work, "b.txt");
    let (workbench, panel, cx) = open(cx, work);

    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("feat"));
    let tooltip = branch_tooltip(cx);
    assert!(!tooltip.contains("origin"), "{tooltip}");
    let shown = header(&panel, cx).expect("the sync header shows");
    assert_eq!(shown.upstream, None);
    assert_eq!(shown.counts, None);
    assert!(cx.debug_bounds("changes-sync-counts").is_none());
    assert_eq!(
        buttons(&panel, cx),
        [
            ("Pull".to_owned(), true),
            ("Publish Branch".to_owned(), true),
            ("Fetch".to_owned(), true),
        ]
    );

    // Publishing sets the upstream, which the widget and header then name.
    click("changes-push", cx);
    assert_eq!(routed(&workbench, cx), [GitCommand::Push]);
    let told = toasts(&workbench, cx);
    assert!(
        matches!(told.as_slice(), [(Level::Info, message, false)] if message.contains("origin/feat")),
        "{told:?}"
    );
    assert_eq!(git(&fixture.origin, &["rev-parse", "feat"]), head(work));
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("feat"));
    let shown = header(&panel, cx).expect("the sync header shows");
    assert_eq!(shown.upstream.as_deref(), Some("origin/feat"));
    assert_eq!(buttons(&panel, cx)[1], ("Push".to_owned(), true));
    let tooltip = branch_tooltip(cx);
    assert!(tooltip.contains("origin/feat"), "{tooltip}");

    // Detached, there's no branch to publish.
    git(work, &["switch", "-q", "--detach"]);
    refresh(&workbench, cx);
    assert_eq!(buttons(&panel, cx)[1], ("Push".to_owned(), true));
    assert_eq!(header(&panel, cx).unwrap().counts, None);
}

#[gpui::test]
fn counts_refresh_after_fetch_pull_push_and_commit(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    let (workbench, panel, cx) = open(cx, work);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));

    // A commit from the panel's box.
    fs::write(work.join("b.txt"), "b\n").unwrap();
    git(work, &["add", "b.txt"]);
    refresh(&workbench, cx);
    let commit_box = panel.read_with(cx, |panel, _| panel.commit_box().cloned());
    let commit_box = commit_box.expect("the panel's drawn with its box");
    cx.update(|_, cx| set_box_text(&commit_box, "local", cx));
    draw(cx);
    let button = cx.debug_bounds("commit-button").expect("drawn");
    cx.simulate_click(button.center(), Modifiers::none());
    draw(cx);
    assert_eq!(git(work, &["log", "-1", "--format=%s"]), "local");
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    assert_eq!(branch_sync(&workbench, cx).as_deref(), Some("↑1"));
    let counts = header(&panel, cx).unwrap().counts;
    assert_eq!(counts.as_deref(), Some("↑1"));

    // A push.
    click("changes-push", cx);
    assert_eq!(git(&fixture.origin, &["rev-parse", "main"]), head(work));
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    assert_eq!(header(&panel, cx).unwrap().counts, None);

    // The remote moves: nothing shows until a fetch, as there's no network
    // but the fetch's.
    move_remote(&fixture, "c.txt");
    refresh(&workbench, cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    click("changes-fetch", cx);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    assert_eq!(branch_sync(&workbench, cx).as_deref(), Some("↓1"));
    let counts = header(&panel, cx).unwrap().counts;
    assert_eq!(counts.as_deref(), Some("↓1"));

    // A pull.
    click("changes-pull", cx);
    assert_eq!(head(work), head(&fixture.other));
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    assert_eq!(header(&panel, cx).unwrap().counts, None);
    assert_eq!(
        routed(&workbench, cx),
        [GitCommand::Push, GitCommand::Fetch, GitCommand::Pull]
    );
}

#[gpui::test]
fn the_panel_header_buttons_run_the_git_commands_and_disable_while_one_runs(
    cx: &mut TestAppContext,
) {
    let fixture = fixture();
    let work = &fixture.work;
    commit_file(work, "b.txt");
    let (workbench, panel, cx) = open(cx, work);
    routed(&workbench, cx);
    let all_on = [
        ("Pull".to_owned(), true),
        ("Push".to_owned(), true),
        ("Fetch".to_owned(), true),
    ];
    assert_eq!(buttons(&panel, cx), all_on);

    // Held while it runs: every button's off, and a click on one does
    // nothing.
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    click("changes-push", cx);
    assert_eq!(routed(&workbench, cx), [GitCommand::Push]);
    assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1);
    let off: Vec<_> = (all_on.iter())
        .map(|(name, _)| (name.clone(), false))
        .collect();
    assert_eq!(buttons(&panel, cx), off);
    click("changes-fetch", cx);
    assert_eq!(routed(&workbench, cx), []);
    assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1);

    // Once it's done they're on again, and its toast says how it went.
    toasts(&workbench, cx);
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(git(&fixture.origin, &["rev-parse", "main"]), head(work));
    assert_eq!(buttons(&panel, cx), all_on);
    let told = toasts(&workbench, cx);
    assert_eq!(told, [(Level::Info, "Pushed".to_owned(), false)]);

    // Each runs its command, the buttons off until it's done. Work started
    // from now on is still held, each until released.
    for (id, command) in [
        ("changes-fetch", GitCommand::Fetch),
        ("changes-pull", GitCommand::Pull),
    ] {
        click(id, cx);
        assert_eq!(routed(&workbench, cx), [command], "{id}");
        assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1, "{id}");
        assert_eq!(buttons(&panel, cx), off, "{id}");
        cx.update(|_, cx| Workbench::release_git_work(0, cx));
        draw(cx);
        assert_eq!(buttons(&panel, cx), all_on, "{id}");
    }
}

#[gpui::test]
fn a_diverged_branch_says_pull_cannot_fast_forward_and_offers_no_merge(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    move_remote(&fixture, "theirs.txt");
    commit_file(work, "ours.txt");
    git(work, &["fetch", "-q"]);
    let local = head(work);
    let (workbench, panel, cx) = open(cx, work);
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    assert_eq!(branch_sync(&workbench, cx).as_deref(), Some("↑1 ↓1"));
    toasts(&workbench, cx);

    click("changes-pull", cx);
    let told = toasts(&workbench, cx);
    let [(Level::Error, message, choices)] = told.as_slice() else {
        panic!("not one error toast: {told:?}");
    };
    assert!(message.contains("can't fast-forward"), "{message}");
    assert!(!choices, "it offers nothing to choose: {message}");
    // Nothing merged or rebased.
    assert_eq!(head(work), local);
    assert!(!work.join(".git/MERGE_HEAD").exists());
    assert_eq!(git(work, &["status", "--porcelain"]), "");
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("main"));
    assert_eq!(branch_sync(&workbench, cx).as_deref(), Some("↑1 ↓1"));
    assert_eq!(buttons(&panel, cx).len(), 3);
}

#[gpui::test]
fn nothing_shows_outside_a_repository(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::write(root.join("a.txt"), "one\n").unwrap();
    let (workbench, panel, cx) = open(cx, &root);

    assert_eq!(branch_item(&workbench, cx), None);
    assert!(cx.debug_bounds("title-branch").is_none());
    assert_eq!(header(&panel, cx), None);
    assert!(cx.debug_bounds("changes-sync").is_none());
    for id in ["changes-pull", "changes-push", "changes-fetch"] {
        let selector = format!("icon-button:{id}");
        assert!(cx.debug_bounds(selector.leak()).is_none(), "{id}");
    }
}

#[gpui::test]
fn the_counts_stay_when_a_long_branch_name_ellipsizes(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    let long = "feature/a-very-long-branch-name-that-cannot-fit-in-the-widget-at-all";
    git(work, &["switch", "-q", "-c", long]);
    git(work, &["push", "-q", "-u", "origin", long]);
    commit_file(work, "b.txt");
    commit_file(work, "c.txt");
    let (workbench, _panel, cx) = open(cx, work);

    assert_eq!(branch_item(&workbench, cx).as_deref(), Some(long));
    assert_eq!(branch_sync(&workbench, cx).as_deref(), Some("↑2"));
    let button = cx.debug_bounds("title-branch").expect("the branch widget");
    let counts = cx.debug_bounds("title-branch-sync").expect("the counts");
    // The name's capped (it's far wider than that uncapped) and ends in an
    // ellipsis; the counts are whole, inside the control, before the chevron.
    assert!(button.size.width <= gpui::px(200.), "{:?}", button.size);
    assert!(counts.size.width > gpui::px(0.) && counts.size.width < gpui::px(40.));
    assert!(
        counts.left() > button.left() + gpui::px(100.),
        "{counts:?} in {button:?}"
    );
    assert!(
        counts.right() < button.right(),
        "the chevron follows: {counts:?} in {button:?}"
    );
    assert!(
        counts.right() > button.right() - gpui::px(40.),
        "{counts:?} in {button:?}"
    );
    // The text the bar drew for the counts, not recomputed from the state.
    let drawn = crate::title_bar::DRAWN.with_borrow(|drawn| drawn.clone());
    assert!(
        drawn
            .iter()
            .any(|(id, text)| *id == "title-branch-sync" && text.as_ref() == "↑2"),
        "{drawn:?}"
    );

    // In a narrow window the name gives way further; the counts keep their
    // width and stay before the chevron, inside the control.
    cx.simulate_resize(gpui::size(gpui::px(300.), gpui::px(400.)));
    draw(cx);
    let narrow_button = cx.debug_bounds("title-branch").expect("the branch widget");
    let narrow = cx
        .debug_bounds("title-branch-sync")
        .expect("the counts, narrow");
    assert!(
        narrow_button.size.width < button.size.width,
        "{narrow_button:?} vs {button:?}"
    );
    assert_eq!(narrow.size.width, counts.size.width);
    assert!(narrow.left() >= narrow_button.left() && narrow.right() < narrow_button.right());
    let gap = narrow_button.right() - narrow.right();
    let wide_gap = button.right() - counts.right();
    assert_eq!(gap, wide_gap, "the chevron's room after the counts");
    let tooltip = branch_tooltip(cx);
    assert!(tooltip.contains("2 to push"), "{tooltip}");
    assert!(tooltip.contains(&format!("origin/{long}")), "{tooltip}");
}

#[gpui::test]
fn a_branch_with_no_upstream_says_so_in_its_tooltip(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    git(work, &["switch", "-q", "-c", "local-only"]);
    let (workbench, _panel, cx) = open(cx, work);

    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("local-only"));
    assert_eq!(branch_sync(&workbench, cx), None);
    let tooltip = branch_tooltip(cx);
    assert!(
        tooltip.contains("No upstream: Push publishes this branch"),
        "{tooltip}"
    );
}
