//! Fetching, pulling and pushing, on repositories built with the `git` CLI in
//! temporary directories, each with a local bare `origin`, so no network:
//! what reaches the remote, the toasts, the question before a force-push, one
//! running at a time, and the open files reloaded after a pull.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use fe_git::RemoteError;
use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{AFTER_FORCE_CHECK, ALREADY_RUNNING, TARGET_CHANGED, failure};
use crate::{
    Launch, Workbench,
    workbench::{
        git_commands::GitCommand,
        toast::{Choices, Level},
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

/// The fixture's three repositories.
struct Fixture {
    _dir: tempfile::TempDir,
    /// What the workbench is rooted at, on main with `a.txt`, tracking
    /// origin's main. Its own config shuts out the developer's where it
    /// matters, as the workbench reads that too.
    work: PathBuf,
    /// The bare remote.
    origin: PathBuf,
    /// Another clone, to move the remote with.
    other: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
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

/// `rev` in `dir`, or nothing if it isn't there.
fn commit_of(dir: &Path, rev: &str) -> String {
    let refs = format!("refs/heads/{rev}");
    git(dir, &["for-each-ref", "--format=%(objectname)", &refs])
}

fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"])
}

/// Commits `text` as `a.txt` from the other clone, brought up to date, and
/// pushes it.
fn move_remote(fixture: &Fixture, text: &str) {
    git(&fixture.other, &["fetch", "-q", "origin"]);
    git(&fixture.other, &["reset", "-q", "--hard", "origin/main"]);
    fs::write(fixture.other.join("a.txt"), text).unwrap();
    commit(&fixture.other, "upstream");
    git(&fixture.other, &["push", "-q", "origin", "main"]);
}

/// A window of a workbench rooted at `root`, focused, with `a.txt` open if
/// it's there.
fn window(cx: &mut TestAppContext, root: &Path) -> (Entity<Workbench>, VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    another_window(cx, root)
}

/// [`window`], in an app already set up.
fn another_window(cx: &mut TestAppContext, root: &Path) -> (Entity<Workbench>, VisualTestContext) {
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
    let cx = &mut vcx;
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let path = root.join("a.txt");
    if path.exists() {
        let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
    }
    (workbench, vcx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn run(workbench: &Entity<Workbench>, command: GitCommand, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_git_command(command, window, cx)
        })
    });
    draw(cx);
}

/// The shown toasts' levels and messages, emptied.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(Level, String)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    let shown = layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| (entry.toast.level, entry.toast.message.clone()))
            .collect()
    });
    cx.update(|_, cx| layer.update(cx, |layer, cx| layer.dismiss_all(cx)));
    shown
}

/// The one toast shown, emptied: its message, if it's an info.
fn info(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let shown = toasts(workbench, cx);
    match shown.as_slice() {
        [(Level::Info, message)] => message.clone(),
        _ => panic!("not one info toast: {shown:?}"),
    }
}

/// The one toast shown, emptied: its message, if it's an error.
fn error(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let shown = toasts(workbench, cx);
    match shown.as_slice() {
        [(Level::Error, message)] => message.clone(),
        _ => panic!("not one error toast: {shown:?}"),
    }
}

/// The question showing, and what it asks.
fn question(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (Choices, String) {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .find_map(|entry| {
                let choices = entry.toast.choices.clone()?;
                Some((choices, entry.toast.message.clone()))
            })
            .expect("a question shows")
    })
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string())
}

#[gpui::test]
fn push_without_upstream_publishes(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (work, origin) = (&fixture.work, &fixture.origin);
    let (workbench, mut cx) = window(cx, work);
    let cx = &mut cx;

    // With one remote, it's the upstream from now on.
    git(work, &["switch", "-q", "-c", "feat"]);
    run(&workbench, GitCommand::Push, cx);
    assert_eq!(
        info(&workbench, cx),
        "Pushed feat to origin and set its upstream to origin/feat"
    );
    assert_eq!(commit_of(origin, "feat"), head(work));
    let upstream = ["rev-parse", "--abbrev-ref", "feat@{upstream}"];
    assert_eq!(git(work, &upstream), "origin/feat");

    // Origin, among several.
    git(work, &["remote", "add", "backup", "../origin.git"]);
    git(work, &["switch", "-q", "-c", "second"]);
    run(&workbench, GitCommand::Push, cx);
    let told = info(&workbench, cx);
    assert!(told.contains("upstream to origin/second"), "{told}");

    // Several, none of them origin: nothing to go on.
    git(work, &["remote", "rename", "origin", "mirror"]);
    git(work, &["switch", "-q", "-c", "third"]);
    run(&workbench, GitCommand::Push, cx);
    let told = error(&workbench, cx);
    assert!(told.contains("several remotes"), "{told}");
    assert!(told.contains("origin"), "{told}");
    assert_eq!(commit_of(origin, "third"), "");

    // The only remote, whatever it's called.
    git(work, &["remote", "remove", "backup"]);
    run(&workbench, GitCommand::Push, cx);
    let told = info(&workbench, cx);
    assert!(told.contains("upstream to mirror/third"), "{told}");
    assert_eq!(commit_of(origin, "third"), head(work));

    // No remote at all.
    git(work, &["remote", "remove", "mirror"]);
    git(work, &["switch", "-q", "-c", "fourth"]);
    run(&workbench, GitCommand::Push, cx);
    let told = error(&workbench, cx);
    assert!(told.contains("git remote add"), "{told}");
}

#[gpui::test]
fn force_push_is_separate(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (work, origin) = (&fixture.work, &fixture.origin);
    let (workbench, mut cx) = window(cx, work);
    let cx = &mut cx;
    let pushed = head(work);
    git(work, &["commit", "-q", "--amend", "-m", "rewritten"]);
    assert_ne!(head(work), pushed);

    // A rewritten branch doesn't go by Git: Push.
    run(&workbench, GitCommand::Push, cx);
    let told = error(&workbench, cx);
    assert!(told.contains("Pull, then push again"), "{told}");
    assert_eq!(commit_of(origin, "main"), pushed);

    // Nor by the force-push, until it's said yes to.
    run(&workbench, GitCommand::PushForceWithLease, cx);
    let (choices, asked) = question(&workbench, cx);
    assert_eq!(asked, "Force-push main to origin/main with lease?");
    assert_eq!(choices.titles(), ["Yes", "No"]);
    assert_eq!(commit_of(origin, "main"), pushed);
    choices.choose(1);
    draw(cx);
    assert_eq!(commit_of(origin, "main"), pushed);
    toasts(&workbench, cx);

    run(&workbench, GitCommand::PushForceWithLease, cx);
    question(&workbench, cx).0.choose(0);
    toasts(&workbench, cx);
    draw(cx);
    assert_eq!(
        info(&workbench, cx),
        "Force-pushed main to origin/main with lease"
    );
    assert_eq!(commit_of(origin, "main"), head(work));

    // The lease holds: the remote moved since the last fetch.
    move_remote(&fixture, "theirs\n");
    let theirs = commit_of(origin, "main");
    run(&workbench, GitCommand::PushForceWithLease, cx);
    question(&workbench, cx).0.choose(0);
    toasts(&workbench, cx);
    draw(cx);
    let told = error(&workbench, cx);
    assert!(told.contains("changed since your last fetch"), "{told}");
    assert_eq!(commit_of(origin, "main"), theirs);
}

/// A yes holds for the branch it was asked about: switching while the question
/// waits rewrites neither branch.
#[gpui::test]
fn a_stale_yes_force_pushes_nothing(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (work, origin) = (&fixture.work, &fixture.origin);
    git(work, &["switch", "-q", "-c", "feat"]);
    git(work, &["push", "-q", "-u", "origin", "feat"]);
    git(work, &["commit", "-q", "--amend", "-m", "rewritten feat"]);
    git(work, &["switch", "-q", "main"]);
    git(work, &["commit", "-q", "--amend", "-m", "rewritten main"]);
    let (main, feat) = (commit_of(origin, "main"), commit_of(origin, "feat"));
    let (workbench, mut cx) = window(cx, work);
    let cx = &mut cx;

    run(&workbench, GitCommand::PushForceWithLease, cx);
    let (choices, asked) = question(&workbench, cx);
    assert_eq!(asked, "Force-push main to origin/main with lease?");
    git(work, &["switch", "-q", "feat"]);
    choices.choose(0);
    toasts(&workbench, cx);
    draw(cx);
    let told = error(&workbench, cx);
    assert!(told.contains(TARGET_CHANGED), "{told}");
    assert_eq!(commit_of(origin, "main"), main);
    assert_eq!(commit_of(origin, "feat"), feat);

    // Nor does it hold for a commit made since.
    run(&workbench, GitCommand::PushForceWithLease, cx);
    let (choices, asked) = question(&workbench, cx);
    assert_eq!(asked, "Force-push feat to origin/feat with lease?");
    git(
        work,
        &["commit", "-q", "--amend", "-m", "rewritten feat again"],
    );
    choices.choose(0);
    toasts(&workbench, cx);
    draw(cx);
    let told = error(&workbench, cx);
    assert!(told.contains(TARGET_CHANGED), "{told}");
    assert_eq!(commit_of(origin, "feat"), feat);

    // Asked again, it's about the branch now.
    run(&workbench, GitCommand::PushForceWithLease, cx);
    question(&workbench, cx).0.choose(0);
    toasts(&workbench, cx);
    draw(cx);
    assert_eq!(
        info(&workbench, cx),
        "Force-pushed feat to origin/feat with lease"
    );
    assert_eq!(commit_of(origin, "main"), main);
    assert_eq!(commit_of(origin, "feat"), head(work));
}

/// A switch landing after the checks have passed changes nothing: what was
/// confirmed goes where it was confirmed for, and the branch switched to stays.
#[gpui::test]
fn a_switch_after_the_checks_pushes_only_what_was_confirmed(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (work, origin) = (&fixture.work, &fixture.origin);
    git(work, &["switch", "-q", "-c", "feat"]);
    git(work, &["push", "-q", "-u", "origin", "feat"]);
    git(work, &["commit", "-q", "--amend", "-m", "rewritten feat"]);
    git(work, &["switch", "-q", "main"]);
    git(work, &["commit", "-q", "--amend", "-m", "rewritten main"]);
    let (confirmed, feat) = (head(work), commit_of(origin, "feat"));
    let (workbench, mut cx) = window(cx, work);
    let cx = &mut cx;

    run(&workbench, GitCommand::PushForceWithLease, cx);
    let (choices, _) = question(&workbench, cx);
    let switched = work.clone();
    AFTER_FORCE_CHECK.lock().unwrap().push((
        work.clone(),
        Box::new(move || {
            git(&switched, &["switch", "-q", "feat"]);
        }),
    ));
    choices.choose(0);
    toasts(&workbench, cx);
    draw(cx);
    assert!(AFTER_FORCE_CHECK.lock().unwrap().is_empty());
    assert_eq!(
        info(&workbench, cx),
        "Force-pushed main to origin/main with lease"
    );
    assert_eq!(commit_of(origin, "main"), confirmed);
    assert_eq!(commit_of(origin, "feat"), feat);
    assert_eq!(git(work, &["branch", "--show-current"]), "feat");
}

/// One at a time holds across windows into one repository, by a subdirectory
/// or a linked worktree, and not across repositories.
#[gpui::test]
fn one_runs_at_a_time_in_a_repository(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (work, origin) = (&fixture.work, &fixture.origin);
    let sub = work.join("sub");
    fs::create_dir(&sub).unwrap();
    let linked = work.parent().unwrap().join("linked");
    let path = linked.to_str().unwrap();
    git(work, &["worktree", "add", "-q", "-b", "linked", path]);
    fs::write(work.join("b.txt"), "b\n").unwrap();
    commit(work, "local");

    let (first, mut first_cx) = window(cx, work);
    cx.update(Workbench::hold_git_work);
    run(&first, GitCommand::Push, &mut first_cx);
    assert_eq!(cx.update(Workbench::git_work_held), 1);
    let pushing = toasts(&first, &mut first_cx);
    assert_eq!(pushing, [(Level::Info, "Pushing…".to_owned())]);

    for root in [&sub, &linked] {
        let (second, mut second_cx) = another_window(cx, root);
        run(&second, GitCommand::Fetch, &mut second_cx);
        assert_eq!(cx.update(Workbench::git_work_held), 1, "{root:?}");
        // It shows in the active window, whichever that is.
        let shown = [
            toasts(&first, &mut first_cx),
            toasts(&second, &mut second_cx),
        ]
        .concat();
        assert_eq!(
            shown,
            [(Level::Info, ALREADY_RUNNING.to_owned())],
            "{root:?}"
        );
    }

    // Another repository goes ahead.
    let elsewhere = self::fixture();
    let (third, mut third_cx) = another_window(cx, &elsewhere.work);
    run(&third, GitCommand::Fetch, &mut third_cx);
    assert_eq!(cx.update(Workbench::git_work_held), 2);
    cx.update(|cx| Workbench::release_git_work(1, cx));

    cx.update(|cx| Workbench::release_git_work(0, cx));
    first_cx.run_until_parked();
    assert_eq!(commit_of(origin, "main"), head(work));
}

/// While one runs, a force-push says so, whatever it would have said about
/// the branch.
#[gpui::test]
fn already_running_comes_before_a_force_pushs_errors(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    let (workbench, mut cx) = window(cx, work);
    let cx = &mut cx;
    cx.update(|_, cx| Workbench::hold_git_work(cx));

    // Publishing a branch with no upstream.
    git(work, &["switch", "-q", "-c", "feat"]);
    run(&workbench, GitCommand::Push, cx);
    toasts(&workbench, cx);
    run(&workbench, GitCommand::PushForceWithLease, cx);
    assert_eq!(info(&workbench, cx), ALREADY_RUNNING);
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    toasts(&workbench, cx);

    // Fetching on a detached HEAD.
    git(work, &["switch", "-q", "--detach"]);
    run(&workbench, GitCommand::Fetch, cx);
    toasts(&workbench, cx);
    run(&workbench, GitCommand::PushForceWithLease, cx);
    assert_eq!(info(&workbench, cx), ALREADY_RUNNING);
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(info(&workbench, cx), "Fetched");
}

#[gpui::test]
fn runs_off_main_thread(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (work, origin) = (&fixture.work, &fixture.origin);
    let (workbench, mut cx) = window(cx, work);
    let cx = &mut cx;
    fs::write(work.join("b.txt"), "b\n").unwrap();
    commit(work, "local");
    let pushed = commit_of(origin, "main");

    // Each is told as it lands, the progress toast gone by then.
    run(&workbench, GitCommand::Fetch, cx);
    assert_eq!(info(&workbench, cx), "Fetched");

    // Held in the background, with the window going on.
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    run(&workbench, GitCommand::Push, cx);
    assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1);
    assert_eq!(info(&workbench, cx), "Pushing…");
    assert_eq!(commit_of(origin, "main"), pushed);

    // Nothing else starts meanwhile, nor asks to.
    for command in [
        GitCommand::Fetch,
        GitCommand::Pull,
        GitCommand::Push,
        GitCommand::PushForceWithLease,
    ] {
        run(&workbench, command, cx);
        assert_eq!(info(&workbench, cx), ALREADY_RUNNING, "{command:?}");
        assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1);
    }

    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(info(&workbench, cx), "Pushed");
    assert_eq!(commit_of(origin, "main"), head(work));

    // And one starts again once it's done.
    run(&workbench, GitCommand::Fetch, cx);
    assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1);
    assert_eq!(info(&workbench, cx), "Fetching…");
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(info(&workbench, cx), "Fetched");
}

#[gpui::test]
fn errors_are_readable(cx: &mut TestAppContext) {
    use GitCommand::{Fetch, Pull, Push, PushForceWithLease};

    let output = || "git said this".to_owned();
    let root = Path::new("/repo");
    let failed = |output: &str| RemoteError::Failed {
        output: output.to_owned(),
    };
    let cases = [
        (
            Fetch,
            RemoteError::NoRemote,
            "no upstream or origin",
            "git remote add",
            None,
        ),
        (
            Pull,
            RemoteError::NoUpstream,
            "no upstream",
            "Run Git: Push",
            None,
        ),
        (
            Pull,
            RemoteError::Detached,
            "HEAD is detached",
            "Switch to a branch",
            None,
        ),
        (
            Pull,
            RemoteError::Conflicts {
                paths: vec![root.join("a.txt"), root.join("dir/b.txt")],
            },
            "conflicts in 2 files",
            "Resolve them",
            Some("a.txt\ndir/b.txt"),
        ),
        (
            Pull,
            RemoteError::Conflicts {
                paths: vec![root.join("a.txt")],
            },
            "conflicts in 1 file.",
            "Resolve them",
            Some("a.txt"),
        ),
        (
            Pull,
            RemoteError::NotFastForward,
            "diverged",
            "Merge or rebase",
            None,
        ),
        (
            Push,
            RemoteError::Rejected { output: output() },
            "has commits you don't",
            "Pull, then push",
            Some("git said this"),
        ),
        (
            PushForceWithLease,
            RemoteError::Rejected { output: output() },
            "changed since your last fetch",
            "Fetch",
            Some("git said this"),
        ),
        (
            Push,
            RemoteError::Auth { output: output() },
            "credentials",
            "credential helper",
            Some("git said this"),
        ),
        (
            Pull,
            failed("error: local changes in the way\n  a.txt"),
            "local changes in the way",
            "Can't pull",
            Some("error: local changes in the way\n  a.txt"),
        ),
        (Fetch, failed(""), "git failed", "Can't fetch", None),
        (
            Fetch,
            RemoteError::Git(fe_git::GitError::GitMissing),
            "Can't fetch",
            "git",
            None,
        ),
    ];
    for (command, remote_error, cause, what_to_do, detail) in cases {
        let toast = failure(command, remote_error, root);
        assert_eq!(toast.level, Level::Error);
        assert!(toast.message.contains(cause), "{}", toast.message);
        assert!(toast.message.contains(what_to_do), "{}", toast.message);
        assert_eq!(toast.detail.as_deref(), detail, "{}", toast.message);
    }

    // Those git really answers, through the command.
    let fixture = fixture();
    let work = &fixture.work;
    let (workbench, mut cx) = window(cx, work);
    let cx = &mut cx;

    // Signing refused says so, and where to run it.
    git(work, &["config", "push.gpgSign", "true"]);
    run(&workbench, GitCommand::Push, cx);
    let told = error(&workbench, cx);
    assert!(told.contains("signing"), "{told}");
    assert!(told.contains("from a terminal"), "{told}");
    git(work, &["config", "push.gpgSign", "false"]);

    git(work, &["switch", "-q", "-c", "feat"]);
    run(&workbench, GitCommand::Pull, cx);
    let told = error(&workbench, cx);
    assert!(told.contains("no upstream"), "{told}");
    // Nothing to ask about.
    run(&workbench, GitCommand::PushForceWithLease, cx);
    let told = error(&workbench, cx);
    assert!(told.contains("no upstream"), "{told}");

    git(work, &["switch", "-q", "--detach"]);
    run(&workbench, GitCommand::Push, cx);
    let told = error(&workbench, cx);
    assert!(told.contains("HEAD is detached"), "{told}");
}

#[gpui::test]
fn pull_uses_fast_forward_only_by_default(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (work, other) = (&fixture.work, &fixture.other);
    let (workbench, mut cx) = window(cx, work);
    let cx = &mut cx;
    let editor = (workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned()))
        .expect("a.txt is open");
    assert_eq!(text(&editor, cx), "one\n");

    // The open file follows the pull.
    move_remote(&fixture, "upstream\n");
    run(&workbench, GitCommand::Pull, cx);
    assert_eq!(info(&workbench, cx), "Pulled");
    assert_eq!(head(work), head(other));
    assert_eq!(text(&editor, cx), "upstream\n");

    // Diverged, it merges nothing.
    move_remote(&fixture, "further\n");
    fs::write(work.join("b.txt"), "b\n").unwrap();
    commit(work, "local");
    let local = head(work);
    run(&workbench, GitCommand::Pull, cx);
    let told = error(&workbench, cx);
    assert!(told.contains("diverged"), "{told}");
    assert_eq!(head(work), local);
    assert_eq!(text(&editor, cx), "upstream\n");
}

/// A new root keeps the tabs but not the watcher, so a pull landing after one
/// still reloads what it changed, and still says how it went.
#[gpui::test]
fn a_pull_landing_after_the_root_changed_reloads(cx: &mut TestAppContext) {
    let fixture = fixture();
    let work = &fixture.work;
    let (workbench, mut cx) = window(cx, work);
    let cx = &mut cx;
    let editor = (workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned()))
        .expect("a.txt is open");
    move_remote(&fixture, "upstream\n");

    cx.update(|_, cx| Workbench::hold_git_work(cx));
    run(&workbench, GitCommand::Pull, cx);
    assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1);
    toasts(&workbench, cx);

    let elsewhere = tempfile::tempdir().unwrap();
    let root = elsewhere.path().to_owned();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(root, window, cx))
    });
    draw(cx);
    assert_eq!(text(&editor, cx), "one\n");

    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(info(&workbench, cx), "Pulled");
    assert_eq!(text(&editor, cx), "upstream\n");
}

/// A fetch moves the remote-tracking branch and nothing else, and leaves
/// pruning to the repository's `fetch.prune`.
#[gpui::test]
fn fetch_leaves_pruning_to_config(cx: &mut TestAppContext) {
    let fixture = fixture();
    let (work, other) = (&fixture.work, &fixture.other);
    git(other, &["push", "-q", "origin", "main:gone"]);
    let (workbench, mut cx) = window(cx, work);
    let cx = &mut cx;
    let start = head(work);

    run(&workbench, GitCommand::Fetch, cx);
    assert_eq!(info(&workbench, cx), "Fetched");
    let tracking = |work: &Path| git(work, &["for-each-ref", "refs/remotes/origin/gone"]);
    assert_ne!(tracking(work), "");

    move_remote(&fixture, "upstream\n");
    git(other, &["push", "-q", "origin", ":gone"]);
    run(&workbench, GitCommand::Fetch, cx);
    assert_eq!(info(&workbench, cx), "Fetched");
    assert_eq!(head(work), start);
    assert_eq!(git(work, &["rev-parse", "origin/main"]), head(other));
    assert_ne!(tracking(work), "");

    git(work, &["config", "fetch.prune", "true"]);
    run(&workbench, GitCommand::Fetch, cx);
    assert_eq!(info(&workbench, cx), "Fetched");
    assert_eq!(tracking(work), "");
}

#[gpui::test]
fn outside_a_repository_answers(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    for command in [
        GitCommand::Fetch,
        GitCommand::Pull,
        GitCommand::Push,
        GitCommand::PushForceWithLease,
    ] {
        run(&workbench, command, cx);
        assert_eq!(
            info(&workbench, cx),
            "Not in a git repository",
            "{command:?}"
        );
    }
}
