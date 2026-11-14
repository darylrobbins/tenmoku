//! The branches menu (T369, Mac T1768): the title bar's branch widget opens
//! it under itself; its search narrows the actions and every section; Update
//! Project fetches without pruning, then fast-forwards, and says so when it
//! can't; a branch's actions check it out, make a branch from it, and ask
//! for a name before a rename and before deleting one that isn't merged; and
//! the keys that move through it and close it. On repositories built with
//! the `git` CLI in temporary directories, each with a local bare `origin`,
//! so no network.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use gpui::{ClipboardItem, Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench,
    workbench::{
        Palette,
        git_commands::{GitCommand, Routed},
        toast::Level,
    },
};

/// When the fixture's commits were made, oldest first, so the sections'
/// newest-first order is fixed.
const INIT: &str = "2026-01-01T00:00:00Z";
const REMOTE: &str = "2026-01-02T00:00:00Z";
const LOGIN: &str = "2026-01-03T00:00:00Z";
const LATER: &str = "2026-01-04T00:00:00Z";

/// Runs `git` in `dir`, with the developer's own config shut out, committing
/// at `date`; its output, trimmed.
fn git_at(dir: &Path, args: &[&str], date: &str) -> String {
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
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn git(dir: &Path, args: &[&str]) -> String {
    git_at(dir, args, LATER)
}

/// Commits a new file `name` in `dir` at `date`.
fn commit_file(dir: &Path, name: &str, date: &str) {
    fs::write(dir.join(name), format!("{name}\n")).unwrap();
    git_at(dir, &["add", "."], date);
    git_at(dir, &["commit", "-q", "-m", name], date);
}

struct Fixture {
    _dir: tempfile::TempDir,
    /// What the workbench is rooted at: on main, tracking origin's main;
    /// `feat-login` a commit ahead of main and never merged, `feat-search`
    /// and `bugfix` at main; checked out main, feat-login, main,
    /// feat-search, main, in that order. Its config prunes on fetch.
    work: PathBuf,
    /// Another clone, to move the remote with; it pushed `feat-remote`.
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
        ("fetch.prune", "true"),
        ("pull.rebase", "false"),
    ] {
        git(&work, &["config", key, value]);
    }
    commit_file(&work, "a.txt", INIT);
    git(&work, &["remote", "add", "origin", "../origin.git"]);
    git(&work, &["push", "-q", "-u", "origin", "main"]);
    git(&base, &["clone", "-q", "origin.git", "other"]);
    let other = base.join("other");
    git(&other, &["switch", "-q", "-c", "feat-remote"]);
    commit_file(&other, "remote.txt", REMOTE);
    git(&other, &["push", "-q", "origin", "feat-remote"]);
    git(&other, &["switch", "-q", "main"]);
    git(&work, &["fetch", "-q"]);
    git(&work, &["branch", "bugfix"]);
    git(&work, &["switch", "-q", "-c", "feat-login"]);
    commit_file(&work, "login.txt", LOGIN);
    git(&work, &["switch", "-q", "main"]);
    git(&work, &["switch", "-q", "-c", "feat-search"]);
    git(&work, &["switch", "-q", "main"]);
    Fixture {
        _dir: dir,
        work,
        other,
    }
}

/// Commits `name` from the other clone, brought up to date, and pushes it
/// to origin's main.
fn move_remote(fixture: &Fixture, name: &str) {
    let other = &fixture.other;
    git(other, &["pull", "-q", "--ff-only", "origin", "main"]);
    commit_file(other, name, LATER);
    git(other, &["push", "-q", "origin", "main"]);
}

fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"])
}

/// HEAD's branch.
fn head_branch(dir: &Path) -> String {
    git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// Whether `dir` has the full ref `name`.
fn has_ref(dir: &Path, name: &str) -> bool {
    !git(dir, &["for-each-ref", "--format=%(refname)", name]).is_empty()
}

/// A workbench rooted at `root` in a headless window, its title bar shown,
/// focused.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
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
    (workbench, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    cx.run_until_parked();
}

/// Opens the menu as the widget's click does, from the palette's command.
fn open_menu(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_git_command(GitCommand::BranchesMenu, window, cx)
        })
    });
    draw(cx);
    assert!(shown(workbench, cx), "the menu shows");
}

fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, cx| workbench.branches_menu(cx).is_some())
}

/// The menu's rows, as `BranchMenu::lines` has them.
fn lines(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    workbench.read_with(cx, |workbench, cx| {
        let menu = workbench.branches_menu(cx).expect("the menu shows");
        menu.read(cx).lines()
    })
}

fn highlighted(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    workbench.read_with(cx, |workbench, cx| {
        let menu = workbench.branches_menu(cx).expect("the menu shows");
        menu.read(cx).highlighted_line()
    })
}

/// The open branch's actions, and the highlighted one.
fn actions(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<(Vec<String>, Option<String>)> {
    workbench.read_with(cx, |workbench, cx| {
        let menu = workbench.branches_menu(cx).expect("the menu shows");
        menu.read(cx).actions()
    })
}

/// The row of `name` in the section titled `title`.
fn row_of(
    workbench: &Entity<Workbench>,
    title: &str,
    name: &str,
    cx: &mut VisualTestContext,
) -> usize {
    workbench.read_with(cx, |workbench, cx| {
        let menu = workbench.branches_menu(cx).expect("the menu shows");
        (menu.read(cx).row_of(title, name)).unwrap_or_else(|| panic!("{name} in {title}"))
    })
}

/// Clicks what the debug selector `selector` drew, where the last frame
/// drew it.
fn click(selector: String, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector.clone().leak());
    let bounds = bounds.unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

/// Moves the highlight, from none, down to `row`.
fn down_to(workbench: &Entity<Workbench>, row: usize, cx: &mut VisualTestContext) {
    assert_eq!(highlighted(workbench, cx), None);
    let lines = lines(workbench, cx);
    let items = (lines[..=row].iter())
        .filter(|line| *line != "-" && !line.starts_with('#'))
        .count();
    for _ in 0..items {
        keys("down", cx);
    }
    assert_eq!(Some(&lines[row]), highlighted(workbench, cx).as_ref());
}

fn click_row(ix: usize, cx: &mut VisualTestContext) {
    click(format!("git-branches-row:{ix}"), cx);
}

/// Clicks the open branch's action titled `title`.
fn click_action(title: &str, cx: &mut VisualTestContext) {
    click(format!("menu-item:{title}"), cx);
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
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

/// What the menu's search field holds.
fn search_text(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    workbench.read_with(cx, |workbench, cx| {
        let menu = workbench.branches_menu(cx).expect("the menu shows");
        menu.read(cx).search_text(cx)
    })
}

fn held(cx: &mut VisualTestContext) -> usize {
    cx.update(|_, cx| Workbench::git_work_held(cx))
}

fn release(cx: &mut VisualTestContext) {
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
}

fn run(workbench: &Entity<Workbench>, command: GitCommand, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_git_command(command, window, cx)
        })
    });
    draw(cx);
}

/// Opens the actions of `name` in the section titled `title`, and clicks
/// the one titled `action`.
fn choose(
    workbench: &Entity<Workbench>,
    title: &str,
    name: &str,
    action: &str,
    cx: &mut VisualTestContext,
) {
    open_menu(workbench, cx);
    let row = row_of(workbench, title, name, cx);
    click_row(row, cx);
    click_action(action, cx);
}

/// The rows of the branch name's picker, while it shows.
fn name_rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
    workbench.read_with(cx, |workbench, cx| workbench.branch_name_rows(cx))
}

/// The whole menu for the fixture.
const ALL: &[&str] = &[
    "Update Project",
    "Commit…",
    "Push",
    "Fetch",
    "-",
    "New Branch…",
    "-",
    "# Recent",
    "  main *",
    "  feat-search",
    "  feat-login",
    "-",
    "# Local",
    "  feat-login",
    "  bugfix",
    "  feat-search",
    "  main *",
    "-",
    "# Remote",
    "## origin",
    "  feat-remote",
    "  main",
];

#[gpui::test]
fn the_widget_click_opens_the_menu(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (workbench, cx) = open(cx, &fixture.work);
    routed(&workbench, cx);

    let widget = cx
        .debug_bounds("title-branch")
        .expect("the widget is drawn");
    cx.simulate_click(widget.center(), Modifiers::none());
    draw(cx);
    assert_eq!(routed(&workbench, cx), [GitCommand::BranchesMenu]);
    assert!(shown(&workbench, cx), "the menu shows");
    let palette = workbench.read_with(cx, |workbench, _| workbench.palette);
    assert_eq!(palette, Some(Palette::Branch));

    // Under the widget, from its left edge.
    let menu = cx
        .debug_bounds("git-branches-menu")
        .expect("the menu is drawn");
    assert!(menu.top() >= widget.bottom(), "{menu:?} under {widget:?}");
    assert!(menu.top() <= widget.bottom() + gpui::px(4.), "{menu:?}");
    assert!(
        (menu.left() - widget.left()).abs() <= gpui::px(1.),
        "{menu:?}"
    );
    assert!(cx.debug_bounds("git-branches-search").is_some());

    // The actions, then Recent, Local and Remote, each newest first.
    assert_eq!(lines(&workbench, cx), ALL);
    assert_eq!(highlighted(&workbench, cx), None);
    assert!(actions(&workbench, cx).is_none());

    // The widget's click again closes it, and once more opens it.
    cx.simulate_click(widget.center(), Modifiers::none());
    draw(cx);
    assert!(!shown(&workbench, cx), "a second click closes it");
    cx.simulate_click(widget.center(), Modifiers::none());
    draw(cx);
    assert!(shown(&workbench, cx), "a third opens it");

    // With an upstream ahead, the branch's counts; with none, Push publishes.
    keys("escape", cx);
    commit_file(&fixture.work, "ahead.txt", LATER);
    git(&fixture.work, &["switch", "-q", "-c", "unpublished"]);
    open_menu(&workbench, cx);
    let lines = lines(&workbench, cx);
    assert_eq!(lines[2], "Publish Branch", "{lines:?}");
    let local = lines.iter().position(|line| line == "# Local").unwrap();
    assert!(
        lines[local..].contains(&"  main ↑1".to_owned()),
        "{lines:?}"
    );
    assert!(
        lines[local..].contains(&"  unpublished *".to_owned()),
        "{lines:?}"
    );

    // `git.switch_branch` still opens the picker.
    keys("escape", cx);
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_git_command(GitCommand::SwitchBranch, window, cx)
        })
    });
    draw(cx);
    assert!(!shown(&workbench, cx));
    let palette = workbench.read_with(cx, |workbench, _| workbench.palette);
    assert_eq!(palette, Some(Palette::Branch));
}

#[gpui::test]
fn filter_narrows_every_section(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (workbench, cx) = open(cx, &fixture.work);
    open_menu(&workbench, cx);

    // What's typed shows in the search row, and narrows each section.
    keys("f e a t", cx);
    assert_eq!(
        lines(&workbench, cx),
        [
            "# Recent",
            "  feat-search",
            "  feat-login",
            "-",
            "# Local",
            "  feat-login",
            "  feat-search",
            "-",
            "# Remote",
            "## origin",
            "  feat-remote",
        ]
    );
    // The first branch is highlighted, for Enter.
    assert_eq!(
        highlighted(&workbench, cx).as_deref(),
        Some("  feat-search")
    );
    keys("- l", cx);
    assert_eq!(
        lines(&workbench, cx),
        ["# Recent", "  feat-login", "-", "# Local", "  feat-login"]
    );
    // Ignoring case.
    keys(
        "backspace backspace backspace backspace backspace backspace",
        cx,
    );
    keys("shift-m a i", cx);
    assert_eq!(
        lines(&workbench, cx),
        [
            "# Recent",
            "  main *",
            "-",
            "# Local",
            "  main *",
            "-",
            "# Remote",
            "## origin",
            "  main",
        ]
    );
    assert_eq!(highlighted(&workbench, cx).as_deref(), Some("  main *"));
    // The actions by title.
    keys("backspace backspace backspace p u s", cx);
    assert_eq!(lines(&workbench, cx), ["Push"]);
    assert_eq!(highlighted(&workbench, cx).as_deref(), Some("Push"));
    // Nothing.
    keys("z", cx);
    assert_eq!(lines(&workbench, cx), ["No branches match"]);
    assert_eq!(highlighted(&workbench, cx), None);
    // Taken back, all of it.
    keys("backspace backspace backspace backspace", cx);
    assert_eq!(lines(&workbench, cx), ALL);
}

#[gpui::test]
fn header_update_runs_fetch_then_ff_pull(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    // A branch gone from origin since the last fetch, which a fetch with
    // the fixture's `fetch.prune` would delete.
    git(work, &["push", "-q", "origin", "bugfix:stale"]);
    git(work, &["fetch", "-q"]);
    git(
        &fixture.other,
        &["push", "-q", "origin", "--delete", "stale"],
    );
    // Origin's main moves on, and a new branch is pushed.
    move_remote(&fixture, "theirs.txt");
    git(&fixture.other, &["push", "-q", "origin", "main:fresh"]);
    let theirs = head(&fixture.other);
    let (workbench, cx) = open(cx, work);
    toasts(&workbench, cx);
    routed(&workbench, cx);

    // Update Project, from the keyboard.
    open_menu(&workbench, cx);
    keys("down", cx);
    assert_eq!(
        highlighted(&workbench, cx).as_deref(),
        Some("Update Project")
    );
    keys("enter", cx);
    assert!(!shown(&workbench, cx), "the menu closes");
    assert_eq!(
        routed(&workbench, cx),
        [GitCommand::BranchesMenu, GitCommand::UpdateProject]
    );
    let told = toasts(&workbench, cx);
    assert_eq!(
        told,
        [(
            Level::Info,
            "Updated main from origin/main".to_owned(),
            false
        )]
    );
    // Fetched, all of origin, without pruning; then fast-forwarded.
    assert_eq!(head(work), theirs);
    assert_eq!(head_branch(work), "main");
    assert!(has_ref(work, "refs/remotes/origin/fresh"), "fetched");
    assert!(has_ref(work, "refs/remotes/origin/stale"), "not pruned");
    assert!(!work.join(".git/MERGE_HEAD").exists());
    assert!(work.join("theirs.txt").exists());

    // Again: nothing to bring in.
    open_menu(&workbench, cx);
    click_row(0, cx);
    let told = toasts(&workbench, cx);
    assert_eq!(
        told,
        [(
            Level::Info,
            "main is already up to date with origin/main".to_owned(),
            false
        )]
    );

    // A fetch that fails stops it before anything's merged.
    move_remote(&fixture, "later.txt");
    git(work, &["remote", "set-url", "origin", "../missing.git"]);
    let before = head(work);
    open_menu(&workbench, cx);
    click_row(0, cx);
    let told = toasts(&workbench, cx);
    let [(Level::Error, message, false)] = told.as_slice() else {
        panic!("not one error toast: {told:?}");
    };
    assert!(message.starts_with("Can't update: "), "{message}");
    assert_eq!(head(work), before);

    // From the palette too.
    git(work, &["remote", "set-url", "origin", "../origin.git"]);
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_git_command(GitCommand::UpdateProject, window, cx)
        })
    });
    draw(cx);
    assert_eq!(head(work), head(&fixture.other));
}

#[gpui::test]
fn branch_submenu_checkout_and_new_branch_from(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    let (workbench, cx) = open(cx, work);
    toasts(&workbench, cx);

    // The current branch's actions.
    open_menu(&workbench, cx);
    let main = row_of(&workbench, "Local", "main", cx);
    click_row(main, cx);
    assert_eq!(
        actions(&workbench, cx),
        Some((
            vec![
                "New Branch from 'main'…".to_owned(),
                "Rename…".to_owned(),
                "Push".to_owned(),
                "Pull".to_owned(),
                "Update".to_owned(),
            ],
            None
        ))
    );
    keys("escape escape", cx);
    assert!(!shown(&workbench, cx));

    // Another local branch's, from the keyboard: Checkout first.
    open_menu(&workbench, cx);
    let login = row_of(&workbench, "Local", "feat-login", cx);
    down_to(&workbench, login, cx);
    keys("right", cx);
    assert_eq!(
        actions(&workbench, cx),
        Some((
            vec![
                "Checkout".to_owned(),
                "New Branch from 'feat-login'…".to_owned(),
                "Merge 'feat-login' into 'main'".to_owned(),
                "Rename…".to_owned(),
                "Delete".to_owned(),
            ],
            Some("Checkout".to_owned())
        ))
    );
    keys("enter", cx);
    assert!(!shown(&workbench, cx));
    assert_eq!(head_branch(work), "feat-login");
    let state = workbench.read_with(cx, |workbench, _| workbench.title_bar_state());
    assert_eq!(state.branch.as_deref(), Some("feat-login"));

    // A remote branch's: Checkout makes a local branch tracking it.
    open_menu(&workbench, cx);
    let remote = row_of(&workbench, "Remote", "feat-remote", cx);
    click(format!("git-branches-chevron:{remote}"), cx);
    let (titles, _) = actions(&workbench, cx).expect("its actions are open");
    assert_eq!(
        titles,
        [
            "Checkout",
            "New Branch from 'origin/feat-remote'…",
            "Merge 'origin/feat-remote' into 'feat-login'",
        ]
    );
    click_action("Checkout", cx);
    assert_eq!(head_branch(work), "feat-remote");
    assert_eq!(
        git(
            work,
            &["rev-parse", "--abbrev-ref", "feat-remote@{upstream}"]
        ),
        "origin/feat-remote"
    );

    // New Branch from a remote branch: asks for the name, then makes it
    // there, tracking it, and switches to it.
    open_menu(&workbench, cx);
    let remote = row_of(&workbench, "Remote", "main", cx);
    click_row(remote, cx);
    click_action("New Branch from 'origin/main'…", cx);
    assert!(!shown(&workbench, cx));
    assert_eq!(name_rows(&workbench, cx), Some(vec![]));
    cx.simulate_input("from-origin");
    draw(cx);
    assert_eq!(
        name_rows(&workbench, cx),
        Some(vec![
            "Create branch 'from-origin' from 'origin/main' and switch to it".to_owned()
        ])
    );
    assert_eq!(head_branch(work), "feat-remote", "nothing made yet");
    keys("enter", cx);
    assert_eq!(head_branch(work), "from-origin");
    assert_eq!(head(work), git(work, &["rev-parse", "origin/main"]));
    assert_eq!(
        git(
            work,
            &["rev-parse", "--abbrev-ref", "from-origin@{upstream}"]
        ),
        "origin/main"
    );

    // From a local one: made at its tip, tracking nothing.
    open_menu(&workbench, cx);
    let login = row_of(&workbench, "Local", "feat-login", cx);
    click_row(login, cx);
    click_action("New Branch from 'feat-login'…", cx);
    cx.simulate_input("from-login");
    keys("enter", cx);
    assert_eq!(head_branch(work), "from-login");
    assert_eq!(head(work), git(work, &["rev-parse", "feat-login"]));
    assert_eq!(
        git(
            work,
            &[
                "config",
                "--default",
                "",
                "--get",
                "branch.from-login.merge"
            ]
        ),
        ""
    );
    assert_eq!(toasts(&workbench, cx), []);
}

#[gpui::test]
fn rename_and_delete_ask_first(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    let (workbench, cx) = open(cx, work);
    toasts(&workbench, cx);
    let branches = |work: &Path| git(work, &["branch", "--format=%(refname:short)"]);

    // Rename asks for the new name, the old one to start with.
    open_menu(&workbench, cx);
    let search = row_of(&workbench, "Local", "feat-search", cx);
    click_row(search, cx);
    click_action("Rename…", cx);
    assert_eq!(
        name_rows(&workbench, cx),
        Some(vec!["Rename 'feat-search' to 'feat-search'".to_owned()])
    );
    for _ in 0.."search".len() {
        cx.simulate_keystrokes("backspace");
    }
    cx.simulate_input("find");
    draw(cx);
    assert_eq!(
        name_rows(&workbench, cx),
        Some(vec!["Rename 'feat-search' to 'feat-find'".to_owned()])
    );
    assert_eq!(branches(work), "bugfix\nfeat-login\nfeat-search\nmain");
    keys("enter", cx);
    assert_eq!(branches(work), "bugfix\nfeat-find\nfeat-login\nmain");
    assert_eq!(
        toasts(&workbench, cx),
        [(
            Level::Info,
            "Renamed feat-search to feat-find".to_owned(),
            false
        )]
    );

    // A branch that isn't merged asks before it's deleted; Cancel keeps it.
    open_menu(&workbench, cx);
    let login = row_of(&workbench, "Local", "feat-login", cx);
    click_row(login, cx);
    click_action("Delete", cx);
    let (question, detail) = cx.pending_prompt().expect("a question");
    assert_eq!(question, "feat-login isn't fully merged.");
    assert!(detail.contains("Deleting it loses"), "{detail}");
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert!(branches(work).contains("feat-login"));
    assert_eq!(toasts(&workbench, cx), []);
    // Delete deletes it.
    open_menu(&workbench, cx);
    let login = row_of(&workbench, "Local", "feat-login", cx);
    click_row(login, cx);
    click_action("Delete", cx);
    cx.simulate_prompt_answer("Delete");
    draw(cx);
    assert_eq!(branches(work), "bugfix\nfeat-find\nmain");
    assert_eq!(
        toasts(&workbench, cx),
        [(Level::Info, "Deleted branch feat-login".to_owned(), false)]
    );

    // A merged one goes without asking.
    open_menu(&workbench, cx);
    let bugfix = row_of(&workbench, "Local", "bugfix", cx);
    click_row(bugfix, cx);
    click_action("Delete", cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!(branches(work), "feat-find\nmain");
    assert_eq!(
        toasts(&workbench, cx),
        [(Level::Info, "Deleted branch bugfix".to_owned(), false)]
    );

    // The current branch has no Delete.
    open_menu(&workbench, cx);
    let main = row_of(&workbench, "Local", "main", cx);
    click_row(main, cx);
    let (titles, _) = actions(&workbench, cx).expect("its actions are open");
    assert!(!titles.contains(&"Delete".to_owned()), "{titles:?}");
}

#[gpui::test]
fn keyboard_navigation_and_escape(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (workbench, cx) = open(cx, &fixture.work);
    let workbench_focus = workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx));
    open_menu(&workbench, cx);

    // Down and Up over the actions and branches, past separators and
    // titles, wrapping.
    let mut seen = Vec::new();
    for _ in 0..8 {
        keys("down", cx);
        seen.push(highlighted(&workbench, cx).unwrap());
    }
    assert_eq!(
        seen,
        [
            "Update Project",
            "Commit…",
            "Push",
            "Fetch",
            "New Branch…",
            "  main *",
            "  feat-search",
            "  feat-login",
        ]
    );
    keys("up up up", cx);
    assert_eq!(highlighted(&workbench, cx).as_deref(), Some("New Branch…"));
    keys("up up up up up", cx);
    assert_eq!(
        highlighted(&workbench, cx).as_deref(),
        Some("  main"),
        "wrapped"
    );
    keys("down", cx);
    assert_eq!(
        highlighted(&workbench, cx).as_deref(),
        Some("Update Project")
    );

    // Right opens a branch's actions; Down and Up move in them; Left and
    // Escape close them, the menu staying.
    keys("up up", cx);
    assert_eq!(
        highlighted(&workbench, cx).as_deref(),
        Some("  feat-remote")
    );
    keys("right", cx);
    let (_, at) = actions(&workbench, cx).expect("its actions are open");
    assert_eq!(at.as_deref(), Some("Checkout"));
    keys("down", cx);
    let (_, at) = actions(&workbench, cx).unwrap();
    assert_eq!(at.as_deref(), Some("New Branch from 'origin/feat-remote'…"));
    keys("up up", cx);
    let (_, at) = actions(&workbench, cx).unwrap();
    assert_eq!(
        at.as_deref(),
        Some("Merge 'origin/feat-remote' into 'main'")
    );
    keys("left", cx);
    assert_eq!(actions(&workbench, cx), None);
    assert_eq!(
        highlighted(&workbench, cx).as_deref(),
        Some("  feat-remote")
    );
    keys("enter", cx);
    assert!(actions(&workbench, cx).is_some(), "Enter opens them too");
    keys("escape", cx);
    assert_eq!(actions(&workbench, cx), None);
    assert!(shown(&workbench, cx));

    // Escape closes the menu, focus going back; nothing typed reached
    // anything else.
    keys("escape", cx);
    assert!(!shown(&workbench, cx));
    let palette = workbench.read_with(cx, |workbench, _| workbench.palette);
    assert_eq!(palette, None);
    assert!(cx.update(|window, cx| workbench_focus.contains_focused(window, cx)));

    // A press outside closes it too.
    open_menu(&workbench, cx);
    let pane = workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).bounds());
    cx.simulate_click(pane.center(), Modifiers::none());
    draw(cx);
    assert!(!shown(&workbench, cx));
}

#[gpui::test]
fn diverged_update_says_cannot_fast_forward(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    move_remote(&fixture, "theirs.txt");
    commit_file(work, "ours.txt", LATER);
    let ours = head(work);
    let theirs = head(&fixture.other);
    let (workbench, cx) = open(cx, work);
    toasts(&workbench, cx);

    // From the current branch's own Update.
    open_menu(&workbench, cx);
    let main = row_of(&workbench, "Local", "main", cx);
    click_row(main, cx);
    click_action("Update", cx);
    let told = toasts(&workbench, cx);
    assert_eq!(
        told,
        [(
            Level::Error,
            "Can't update: the branch and its upstream have diverged, so it can't fast-forward. \
             Merge or rebase them from a terminal."
                .to_owned(),
            false
        )],
        "no merge or rebase offered"
    );
    // Fetched, but nothing merged or rebased.
    assert_eq!(git(work, &["rev-parse", "origin/main"]), theirs);
    assert_eq!(head(work), ours);
    assert!(!work.join(".git/MERGE_HEAD").exists());
    assert!(!work.join(".git/rebase-merge").exists());
    assert_eq!(git(work, &["status", "--porcelain"]), "");
    let state = workbench.read_with(cx, |workbench, _| workbench.title_bar_state());
    let crate::title_bar::Tracking::Upstream(upstream) = state.tracking else {
        panic!("main tracks origin/main");
    };
    assert_eq!((upstream.ahead, upstream.behind), (1, 1));
}

#[gpui::test]
fn branch_work_waits_its_turn_behind_queued_work(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    let (workbench, cx) = open(cx, work);
    toasts(&workbench, cx);
    let main = head(work);

    // Update Project starts and is held; a merge asked for meanwhile waits
    // behind it, unstarted.
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    run(&workbench, GitCommand::UpdateProject, cx);
    assert_eq!(held(cx), 1);
    choose(
        &workbench,
        "Local",
        "feat-login",
        "Merge 'feat-login' into 'main'",
        cx,
    );
    assert_eq!(held(cx), 1, "the merge hasn't started");
    assert_eq!(head(work), main);

    // The update lands, then the merge starts.
    release(cx);
    assert_eq!(
        toasts(&workbench, cx),
        [(
            Level::Info,
            "main is already up to date with origin/main".to_owned(),
            false
        )]
    );
    assert_eq!(held(cx), 1, "the merge has started");
    assert_eq!(head(work), main);
    release(cx);
    assert_eq!(head(work), git(work, &["rev-parse", "feat-login"]));
    assert_eq!(
        toasts(&workbench, cx),
        [(Level::Info, "Merged feat-login into main".to_owned(), false)]
    );
}

#[gpui::test]
fn update_merges_nothing_if_the_branch_changes_while_it_fetches(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = fixture.work.clone();
    move_remote(&fixture, "theirs.txt");
    let (main, login) = (head(&work), git(&work, &["rev-parse", "feat-login"]));
    let (workbench, cx) = open(cx, &work);
    toasts(&workbench, cx);

    // Once it's fetched, a terminal switches to another branch.
    let switched = work.clone();
    let hook = Box::new(move || {
        git(&switched, &["switch", "-q", "feat-login"]);
    });
    (crate::workbench::git_commands::git_remote::AFTER_UPDATE_FETCH.lock())
        .unwrap()
        .push((work.clone(), hook));
    run(&workbench, GitCommand::UpdateProject, cx);
    assert_eq!(
        toasts(&workbench, cx),
        [(
            Level::Error,
            "Can't update: the branch or its upstream changed while it fetched, so nothing was \
             merged; run it again"
                .to_owned(),
            false
        )]
    );
    // Fetched, and nothing merged into either branch.
    assert_eq!(
        git(&work, &["rev-parse", "origin/main"]),
        head(&fixture.other)
    );
    assert_eq!(head_branch(&work), "feat-login");
    assert_eq!(head(&work), login);
    assert_eq!(git(&work, &["rev-parse", "main"]), main);
}

#[gpui::test]
fn delete_asks_again_once_the_branch_has_moved(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    let (workbench, cx) = open(cx, work);
    toasts(&workbench, cx);

    choose(&workbench, "Local", "feat-login", "Delete", cx);
    assert!(cx.has_pending_prompt());
    // While it's asked, the branch gets another commit, from elsewhere.
    let tree = git(work, &["rev-parse", "feat-login^{tree}"]);
    let moved = git(
        work,
        &["commit-tree", "-p", "feat-login", "-m", "more", &tree],
    );
    git(work, &["update-ref", "refs/heads/feat-login", &moved]);

    // The yes was for the old tip: nothing's deleted, and it asks again.
    cx.simulate_prompt_answer("Delete");
    draw(cx);
    assert_eq!(git(work, &["rev-parse", "feat-login"]), moved);
    let (question, _) = cx.pending_prompt().expect("it asks again");
    assert_eq!(question, "feat-login isn't fully merged.");
    cx.simulate_prompt_answer("Delete");
    draw(cx);
    assert!(!has_ref(work, "refs/heads/feat-login"));
    assert_eq!(
        toasts(&workbench, cx),
        [(Level::Info, "Deleted branch feat-login".to_owned(), false)]
    );
}

#[gpui::test]
fn a_merge_landing_after_the_root_changed_reloads_the_open_files(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    git(work, &["switch", "-q", "feat-login"]);
    fs::write(work.join("a.txt"), "login\n").unwrap();
    git(work, &["commit", "-q", "-am", "login a"]);
    git(work, &["switch", "-q", "main"]);
    let (workbench, cx) = open(cx, work);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = work.join("a.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor: Entity<EditorView> = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a.txt is open");
    let text = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string())
    };
    assert_eq!(text(cx), "a.txt\n");

    // The merge is held while the root changes.
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    choose(
        &workbench,
        "Local",
        "feat-login",
        "Merge 'feat-login' into 'main'",
        cx,
    );
    assert_eq!(held(cx), 1);
    let elsewhere = tempfile::tempdir().unwrap();
    let root = elsewhere.path().to_owned();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(root, window, cx))
    });
    draw(cx);
    release(cx);
    assert_eq!(head(work), git(work, &["rev-parse", "feat-login"]));
    assert_eq!(text(cx), "login\n", "the kept tab reloads");
}

#[gpui::test]
fn closing_gives_focus_back_where_it_was(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (workbench, cx) = open(cx, &fixture.work);
    toasts(&workbench, cx);
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    let panel_focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| panel.focus_handle(cx).contains_focused(window, cx))
    };
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    draw(cx);
    assert!(panel_focused(cx));

    // Escape.
    open_menu(&workbench, cx);
    assert!(!panel_focused(cx), "the search field has focus");
    keys("escape", cx);
    assert!(!shown(&workbench, cx));
    assert!(panel_focused(cx), "back to the project tree");

    // The widget's click again.
    open_menu(&workbench, cx);
    let widget = cx
        .debug_bounds("title-branch")
        .expect("the widget is drawn");
    cx.simulate_click(widget.center(), Modifiers::none());
    draw(cx);
    assert!(!shown(&workbench, cx));
    assert!(panel_focused(cx));

    // An action that shows nothing of its own: Fetch.
    open_menu(&workbench, cx);
    let fetch = lines(&workbench, cx)
        .iter()
        .position(|line| line == "Fetch")
        .unwrap();
    click_row(fetch, cx);
    assert!(!shown(&workbench, cx));
    assert!(panel_focused(cx));
    assert_eq!(
        toasts(&workbench, cx),
        [(Level::Info, "Fetched".to_owned(), false)]
    );
}

#[gpui::test]
fn search_takes_pasted_and_composed_text(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (workbench, cx) = open(cx, &fixture.work);
    open_menu(&workbench, cx);

    // Pasted.
    cx.write_to_clipboard(ClipboardItem::new_string("feat-l".to_owned()));
    keys("cmd-v", cx);
    assert_eq!(search_text(&workbench, cx), "feat-l");
    assert_eq!(
        lines(&workbench, cx),
        ["# Recent", "  feat-login", "-", "# Local", "  feat-login"]
    );

    // Text the platform's input commits, not only typed keys.
    for _ in 0.."feat-l".len() {
        keys("backspace", cx);
    }
    assert_eq!(search_text(&workbench, cx), "");
    cx.simulate_input("bü");
    draw(cx);
    assert_eq!(search_text(&workbench, cx), "bü");
    assert_eq!(lines(&workbench, cx), ["No branches match"]);
    // "b": the action's title holds it too.
    keys("backspace", cx);
    assert_eq!(
        lines(&workbench, cx),
        ["New Branch…", "-", "# Local", "  bugfix"]
    );
}
