//! Switching and creating branches, on repositories built with the `git`
//! CLI in temporary directories, each with a local bare `origin`: the
//! status bar's branch item, the branch picker, creating a branch from its
//! name's picker or the vim line, and the question when a switch would
//! overwrite local changes.

use std::{fs, path::Path, process::Command};

use fe_gpui::{EditorSettings, EditorTheme, EditorView, set_vim_enabled};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln_shell::Picker;

use super::{BranchPicker, CREATE_ROW, ITEM, NamePicker, OVERWRITE};
use crate::{
    Launch, StatusSide, Workbench,
    workbench::{Palette, git_commands::GitCommand},
};

fn set_root(workbench: &Entity<Workbench>, root: &Path, cx: &mut VisualTestContext) {
    let root = root.to_owned();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.set_root(root, window, cx))
    });
    draw(cx);
}

fn focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// The base text of `a.txt` on main, and on feat.
pub(super) const MAIN_TEXT: &str = "one\n";
pub(super) const FEAT_TEXT: &str = "feat\n";

/// Runs `git` in `dir`, with the developer's own config shut out, and
/// returns what it printed, trimmed.
pub(super) fn git(dir: &Path, args: &[&str]) -> String {
    git_at(dir, args, "2026-01-01T00:00:00Z")
}

/// Runs `git` as [`git`] does, committing at `date`.
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

/// A directory holding `work`, a repository on main with `a.txt`, and a
/// branch feat a day newer that changes it; and `origin.git`, a bare
/// repository it pushed feat to, so `origin/feat` is its one
/// remote-tracking branch.
pub(super) fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    git(base, &["init", "-q", "--bare", "origin.git"]);
    let work = base.join("work");
    fs::create_dir(&work).unwrap();
    git(&work, &["init", "-q"]);
    // For the git fastedit runs, which the helper's `-c`s don't reach.
    git(&work, &["config", "user.name", "t"]);
    git(&work, &["config", "user.email", "t@t"]);
    git(&work, &["config", "commit.gpgsign", "false"]);
    fs::write(work.join("a.txt"), MAIN_TEXT).unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-q", "-m", "init"]);
    git(&work, &["switch", "-q", "-c", "feat"]);
    fs::write(work.join("a.txt"), FEAT_TEXT).unwrap();
    git_at(
        &work,
        &["commit", "-q", "-am", "feat work"],
        "2026-01-02T00:00:00Z",
    );
    git(&work, &["switch", "-q", "main"]);
    let origin = base.join("origin.git");
    git(
        &work,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&work, &["push", "-q", "origin", "feat"]);
    dir
}

pub(super) fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
}

/// A window of a workbench rooted at `root`, focused, with vim on, and its
/// `a.txt` open if it has one.
pub(super) fn window(
    cx: &mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, VisualTestContext) {
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
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    (workbench, vcx)
}

/// Lets background work land, then draws a frame.
pub(super) fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Reads git's status again, as a watcher batch would have it.
fn refresh(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let git = workbench.read_with(cx, |workbench, _| workbench.git().clone());
    git.update(cx, |git, cx| git.refresh(cx));
    draw(cx);
}

/// The branch item's text, as the bar last drew it.
fn branch_item(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let bar = workbench.read_with(cx, |workbench, _| workbench.status_bar().clone());
    bar.read_with(cx, |bar, _| bar.drawn(StatusSide::Left))
        .into_iter()
        .find(|(id, _)| id == ITEM)
        .map(|(_, text)| text.to_string())
}

/// The picker of type `D` showing as the branch pickers' modal, if one is.
fn modal<D: crate::PickerDelegate>(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<Picker<D>>> {
    workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        if layer.modal() != Some(Palette::Branch.id()) {
            return None;
        }
        layer.modal_view()?.clone().downcast().ok()
    })
}

/// The branch picker's rows, each with whether it's marked current.
fn rows(picker: &Entity<Picker<BranchPicker>>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
    picker.read_with(cx, |picker, _| picker.delegate.labels())
}

fn selected<D: crate::PickerDelegate>(
    picker: &Entity<Picker<D>>,
    cx: &mut VisualTestContext,
) -> usize {
    picker.read_with(cx, |picker, _| picker.selected())
}

fn run(workbench: &Entity<Workbench>, command: GitCommand, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_git_command(command, window, cx)
        })
    });
    draw(cx);
}

/// Types `:{line}` and Enter.
pub(super) fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor is active")
}

/// `editor`'s vim message, emptied.
fn message(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            let message = editor.vim_state()?.message().map(str::to_owned);
            editor.set_vim_message("", cx);
            message.filter(|message| !message.is_empty())
        })
    })
}

fn head(work: &Path) -> String {
    git(work, &["rev-parse", "--abbrev-ref", "HEAD"])
}

pub(super) fn stashes(work: &Path) -> usize {
    git(work, &["stash", "list"]).lines().count()
}

#[gpui::test]
fn status_item(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    init(cx);
    let (workbench, mut vcx) = window(cx, &work);
    let wcx = &mut vcx;
    assert_eq!(branch_item(&workbench, wcx).as_deref(), Some("main"));

    // An edit on disk marks it.
    fs::write(work.join("a.txt"), "edited\n").unwrap();
    refresh(&workbench, wcx);
    assert_eq!(branch_item(&workbench, wcx).as_deref(), Some("main*"));
    fs::write(work.join("a.txt"), MAIN_TEXT).unwrap();
    refresh(&workbench, wcx);
    assert_eq!(branch_item(&workbench, wcx).as_deref(), Some("main"));

    // A switch from a terminal shows after the next refresh.
    git(&work, &["switch", "-q", "feat"]);
    refresh(&workbench, wcx);
    assert_eq!(branch_item(&workbench, wcx).as_deref(), Some("feat"));

    // Detached: the short id.
    git(&work, &["switch", "-q", "--detach", "main"]);
    refresh(&workbench, wcx);
    let id = git(&work, &["rev-parse", "main"]);
    assert_eq!(branch_item(&workbench, wcx), Some(id[..7].to_owned()));
    git(&work, &["switch", "-q", "main"]);
    refresh(&workbench, wcx);

    // A click opens the branch picker.
    assert!(modal::<BranchPicker>(&workbench, wcx).is_none());
    let bounds = wcx
        .debug_bounds("status-item:git.branch")
        .expect("the item is drawn");
    wcx.simulate_click(bounds.center(), Modifiers::none());
    draw(wcx);
    let picker = modal::<BranchPicker>(&workbench, wcx).expect("the branch picker shows");
    assert_eq!(rows(&picker, wcx)[0].0, CREATE_ROW);

    // Outside a repository, no item.
    let outside = tempfile::tempdir().unwrap();
    let (other, mut ocx) = window(cx, outside.path());
    let ocx = &mut ocx;
    refresh(&other, ocx);
    assert_eq!(branch_item(&other, ocx), None);
    let bar = other.read_with(ocx, |workbench, _| workbench.status_bar().clone());
    let left = bar.read_with(ocx, |bar, _| bar.drawn(StatusSide::Left));
    assert!(left.iter().all(|(id, _)| id != ITEM), "{left:?}");
}

#[gpui::test]
fn picker(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    init(cx);
    let (workbench, mut cx) = window(cx, &work);
    let cx = &mut cx;
    let editor = active(&workbench, cx);

    ex("Git switch", cx);
    let picker = modal::<BranchPicker>(&workbench, cx).expect("the branch picker shows");
    let row = |name: &str, current| (name.to_owned(), current);
    assert_eq!(
        rows(&picker, cx),
        [
            row(CREATE_ROW, false),
            row("feat", false),
            row("main", true),
            row("origin/feat", false),
        ]
    );

    // ctrl-n and ctrl-p move the selection.
    assert_eq!(selected(&picker, cx), 0);
    cx.simulate_keystrokes("ctrl-n ctrl-n ctrl-n");
    assert_eq!(selected(&picker, cx), 3);
    cx.simulate_keystrokes("ctrl-p ctrl-p");
    assert_eq!(selected(&picker, cx), 1);

    // Typing filters by name; the create row stays.
    cx.simulate_input("fe");
    draw(cx);
    let names: Vec<String> = rows(&picker, cx)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, [CREATE_ROW, "feat", "origin/feat"]);

    // Enter on feat switches to it, and focus goes back to the editor.
    cx.simulate_keystrokes("ctrl-n enter");
    draw(cx);
    assert!(modal::<BranchPicker>(&workbench, cx).is_none());
    assert_eq!(head(&work), "feat");
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("feat"));
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));

    // On origin/feat with no local feat: a local feat tracking it.
    git(&work, &["switch", "-q", "main"]);
    git(&work, &["branch", "-q", "-D", "feat"]);
    run(&workbench, GitCommand::SwitchBranch, cx);
    let picker = modal::<BranchPicker>(&workbench, cx).expect("the branch picker shows");
    let names: Vec<String> = rows(&picker, cx)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, [CREATE_ROW, "main", "origin/feat"]);
    cx.simulate_keystrokes("ctrl-n ctrl-n enter");
    draw(cx);
    assert_eq!(head(&work), "feat");
    let upstream = git(&work, &["rev-parse", "--abbrev-ref", "feat@{upstream}"]);
    assert_eq!(upstream, "origin/feat");
    assert_eq!(message(&editor, cx), None);
}

#[gpui::test]
fn create(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    init(cx);
    let (workbench, mut cx) = window(cx, &work);
    let cx = &mut cx;
    let editor = active(&workbench, cx);
    let branches = |work: &Path| git(work, &["branch", "--format=%(refname:short)"]);

    // An invalid name: said under the input, and Enter creates nothing.
    run(&workbench, GitCommand::CreateBranch, cx);
    let picker = modal::<NamePicker>(&workbench, cx).expect("the name's picker shows");
    let status = |cx: &mut VisualTestContext| {
        picker.read_with(cx, |picker, _| {
            crate::PickerDelegate::status(&picker.delegate).map(|status| status.to_string())
        })
    };
    assert_eq!(status(cx), None);
    cx.simulate_input("bad..name");
    draw(cx);
    assert_eq!(status(cx).as_deref(), Some("Not a valid branch name"));
    let labels = picker.read_with(cx, |picker, _| picker.delegate.labels());
    assert!(labels.is_empty(), "{labels:?}");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(modal::<NamePicker>(&workbench, cx).is_some());
    assert_eq!(branches(&work), "feat\nmain");
    assert_eq!(head(&work), "main");

    // A valid one creates the branch and switches to it.
    for _ in 0.."bad..name".len() {
        cx.simulate_keystrokes("backspace");
    }
    cx.simulate_input("topic");
    draw(cx);
    assert_eq!(status(cx), None);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(modal::<NamePicker>(&workbench, cx).is_none());
    assert_eq!(head(&work), "topic");
    assert_eq!(branch_item(&workbench, cx).as_deref(), Some("topic"));
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));

    // The picker's first row asks for the name too.
    run(&workbench, GitCommand::SwitchBranch, cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(modal::<BranchPicker>(&workbench, cx).is_none());
    assert!(modal::<NamePicker>(&workbench, cx).is_some());
    cx.simulate_keystrokes("escape escape");
    draw(cx);
    assert!(modal::<NamePicker>(&workbench, cx).is_none());
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));

    // On the vim line.
    ex("Git switch -c topic2", cx);
    assert_eq!(head(&work), "topic2");
    ex("Git checkout -b topic3", cx);
    assert_eq!(head(&work), "topic3");
    ex("Git switch main", cx);
    assert_eq!(head(&work), "main");
    assert_eq!(message(&editor, cx), None);
    ex("Git switch nope", cx);
    assert_eq!(head(&work), "main");
    assert_eq!(message(&editor, cx).as_deref(), Some("Not found: nope"));
    ex("Git switch -c bad..name", cx);
    assert_eq!(
        message(&editor, cx).as_deref(),
        Some("Not a valid branch name")
    );
    assert_eq!(branches(&work), "feat\nmain\ntopic\ntopic2\ntopic3");
}

#[gpui::test]
fn would_overwrite(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    init(cx);
    let (workbench, mut cx) = window(cx, &work);
    let cx = &mut cx;
    let edited = "mine\n";
    fs::write(work.join("a.txt"), edited).unwrap();

    // Cancel changes nothing.
    ex("Git switch feat", cx);
    let (question, detail) = cx.pending_prompt().expect("a question");
    assert_eq!(question, OVERWRITE);
    assert_eq!(detail, "a.txt");
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert_eq!(head(&work), "main");
    assert_eq!(fs::read_to_string(work.join("a.txt")).unwrap(), edited);
    assert_eq!(stashes(&work), 0);

    // Stash & Checkout leaves one stash, and switches.
    ex("Git switch feat", cx);
    assert_eq!(
        cx.pending_prompt().map(|(question, _)| question).as_deref(),
        Some(OVERWRITE)
    );
    cx.simulate_prompt_answer("Stash & Checkout");
    draw(cx);
    assert_eq!(head(&work), "feat");
    assert_eq!(stashes(&work), 1);
    assert_eq!(fs::read_to_string(work.join("a.txt")).unwrap(), FEAT_TEXT);

    // Force Checkout switches, dropping the edit.
    git(&work, &["switch", "-q", "main"]);
    fs::write(work.join("a.txt"), edited).unwrap();
    ex("Git switch feat", cx);
    assert_eq!(
        cx.pending_prompt().map(|(question, _)| question).as_deref(),
        Some(OVERWRITE)
    );
    cx.simulate_prompt_answer("Force Checkout");
    draw(cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!(head(&work), "feat");
    assert_eq!(stashes(&work), 1);
    assert_eq!(fs::read_to_string(work.join("a.txt")).unwrap(), FEAT_TEXT);
    // The open file reloads.
    let editor = active(&workbench, cx);
    let text = editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string());
    assert_eq!(text, FEAT_TEXT);
}

#[test]
fn overwrite_detail_lists_ten() {
    let paths: Vec<String> = (0..12).map(|ix| format!("f{ix}")).collect();
    let detail = super::overwrite_detail(&paths);
    let lines: Vec<&str> = detail.lines().collect();
    assert_eq!(lines.len(), 11);
    assert_eq!(lines[9], "f9");
    assert_eq!(lines[10], "and 2 more");
}

#[test]
fn branch_names() {
    for name in ["topic", "feat/x", "a.b", "a-b_c", "@"] {
        assert!(super::is_valid_name(name), "{name}");
    }
    let invalid = [
        "",
        "bad..name",
        "-x",
        "a b",
        "a~",
        "a^",
        "a:",
        "a?",
        "a*",
        "a[",
        "a\\b",
        ".x",
        "x/.y",
        "x.lock",
        "x/",
        "/x",
        "a//b",
        "x.",
        "a@{b",
        "HEAD",
    ];
    for name in invalid {
        assert!(!super::is_valid_name(name), "{name}");
    }
}

#[gpui::test]
fn root_change(cx: &mut TestAppContext) {
    // Two repositories with the same branches.
    let (one, two) = (fixture(), fixture());
    let (a, b) = (one.path().join("work"), two.path().join("work"));
    init(cx);
    let (workbench, mut cx) = window(cx, &a);
    let cx = &mut cx;

    // A new root dismisses the picker listing the old one's branches.
    ex("Git switch", cx);
    assert!(modal::<BranchPicker>(&workbench, cx).is_some());
    set_root(&workbench, &b, cx);
    assert!(modal::<BranchPicker>(&workbench, cx).is_none());
    set_root(&workbench, &a, cx);

    // An answer to the question about the old root does nothing in either.
    let edited = "mine\n";
    fs::write(a.join("a.txt"), edited).unwrap();
    fs::write(b.join("a.txt"), edited).unwrap();
    ex("Git switch feat", cx);
    assert_eq!(
        cx.pending_prompt().map(|(question, _)| question).as_deref(),
        Some(OVERWRITE)
    );
    set_root(&workbench, &b, cx);
    cx.simulate_prompt_answer("Force Checkout");
    draw(cx);
    for work in [&a, &b] {
        assert_eq!(head(work), "main");
        assert_eq!(fs::read_to_string(work.join("a.txt")).unwrap(), edited);
    }
    assert!(!cx.has_pending_prompt());

    // A listing that lands after the root changed shows nothing.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_git_command(GitCommand::SwitchBranch, window, cx);
            workbench.set_root(a.clone(), window, cx);
        })
    });
    draw(cx);
    assert!(modal::<BranchPicker>(&workbench, cx).is_none());
}

#[gpui::test]
fn superseded(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    init(cx);
    let (workbench, mut cx) = window(cx, &work);
    let cx = &mut cx;
    let editor = active(&workbench, cx);

    // The name's picker, opened while the branches are listed, stays.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_git_command(GitCommand::SwitchBranch, window, cx);
            workbench.run_git_command(GitCommand::CreateBranch, window, cx);
        })
    });
    draw(cx);
    assert!(modal::<BranchPicker>(&workbench, cx).is_none());
    let picker = modal::<NamePicker>(&workbench, cx).expect("the name's picker shows");
    cx.simulate_input("x");
    draw(cx);
    assert_eq!(modal::<NamePicker>(&workbench, cx), Some(picker));
    cx.simulate_keystrokes("escape escape");
    draw(cx);
    assert!(focused(&editor, cx));

    // Two listings: one picker.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_git_command(GitCommand::SwitchBranch, window, cx);
            workbench.run_git_command(GitCommand::SwitchBranch, window, cx);
        })
    });
    draw(cx);
    let first = modal::<BranchPicker>(&workbench, cx).expect("the branch picker shows");
    cx.simulate_input("ma");
    draw(cx);
    assert_eq!(modal::<BranchPicker>(&workbench, cx), Some(first));

    // Replaced by another branch picker, then by the name's: dismissing
    // that one focuses the editor, not a picker that's gone.
    run(&workbench, GitCommand::SwitchBranch, cx);
    assert!(modal::<BranchPicker>(&workbench, cx).is_some());
    run(&workbench, GitCommand::CreateBranch, cx);
    assert!(modal::<NamePicker>(&workbench, cx).is_some());
    cx.simulate_keystrokes("escape escape");
    draw(cx);
    assert!(modal::<NamePicker>(&workbench, cx).is_none());
    assert!(focused(&editor, cx));
}

#[gpui::test]
fn superseded_by_a_modal_since(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    init(cx);
    let (workbench, mut cx) = window(cx, &work);
    let cx = &mut cx;
    let editor = active(&workbench, cx);

    // The name's picker, opened and dismissed while the branches are
    // listed: the listing shows nothing.
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_git_command(GitCommand::SwitchBranch, window, cx);
            workbench.run_git_command(GitCommand::CreateBranch, window, cx);
        })
    });
    cx.simulate_keystrokes("escape escape");
    draw(cx);
    assert!(modal::<BranchPicker>(&workbench, cx).is_none());
    assert!(modal::<NamePicker>(&workbench, cx).is_none());
    assert!(focused(&editor, cx));

    // Quick open, the same palette each time, opened and dismissed twice.
    for _ in 0..2 {
        run(&workbench, GitCommand::SwitchBranch, cx);
        cx.simulate_keystrokes("escape escape");
        draw(cx);
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.run_git_command(GitCommand::SwitchBranch, window, cx)
            })
        });
        cx.simulate_keystrokes("cmd-p");
        cx.simulate_keystrokes("escape escape");
        draw(cx);
        assert!(modal::<BranchPicker>(&workbench, cx).is_none());
        assert!(focused(&editor, cx));
    }
}

#[gpui::test]
fn palettes_replace_them(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    init(cx);
    let (workbench, mut cx) = window(cx, &work);
    let cx = &mut cx;
    let editor = active(&workbench, cx);
    let shown = |workbench: &Entity<Workbench>, cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            workbench.shell().read(cx).modals().read(cx).modal()
        })
    };

    // Quick open over either picker: dismissing it focuses the editor.
    for command in [GitCommand::SwitchBranch, GitCommand::CreateBranch] {
        run(&workbench, command, cx);
        assert_eq!(shown(&workbench, cx), Some(Palette::Branch.id()));
        cx.simulate_keystrokes("cmd-p");
        draw(cx);
        assert_eq!(shown(&workbench, cx), Some(Palette::QuickOpen.id()));
        cx.simulate_keystrokes("escape escape");
        draw(cx);
        assert_eq!(shown(&workbench, cx), None);
        assert!(focused(&editor, cx), "{command:?}");
    }

    // And either picker over quick open.
    for command in [GitCommand::SwitchBranch, GitCommand::CreateBranch] {
        cx.simulate_keystrokes("cmd-p");
        draw(cx);
        run(&workbench, command, cx);
        assert_eq!(shown(&workbench, cx), Some(Palette::Branch.id()));
        cx.simulate_keystrokes("escape escape");
        draw(cx);
        assert_eq!(shown(&workbench, cx), None);
        assert!(focused(&editor, cx), "{command:?}");
    }
}

/// A local branch named `origin/feat`, beside the remote-tracking one.
#[gpui::test]
fn local_named_like_remote(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    git(&work, &["branch", "-q", "-D", "feat"]);
    git(&work, &["branch", "-q", "origin/feat"]);
    init(cx);
    let (workbench, mut cx) = window(cx, &work);
    let cx = &mut cx;
    let head = |work: &Path| git(work, &["symbolic-ref", "HEAD"]);

    run(&workbench, GitCommand::SwitchBranch, cx);
    let picker = modal::<BranchPicker>(&workbench, cx).expect("the branch picker shows");
    let names: Vec<String> = rows(&picker, cx)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, [CREATE_ROW, "main", "origin/feat", "origin/feat"]);
    let ids = picker.read_with(cx, |picker, _| picker.delegate.ids());
    assert_eq!(
        ids,
        [
            "create",
            "refs/heads/main",
            "refs/heads/origin/feat",
            "refs/remotes/origin/feat"
        ]
    );

    // The remote one, by keyboard: a local feat tracking it.
    cx.simulate_keystrokes("ctrl-n ctrl-n ctrl-n enter");
    draw(cx);
    assert_eq!(head(&work), "refs/heads/feat");
    let upstream = git(
        &work,
        &["rev-parse", "--symbolic-full-name", "feat@{upstream}"],
    );
    assert_eq!(upstream, "refs/remotes/origin/feat");

    // The local one, by mouse.
    run(&workbench, GitCommand::SwitchBranch, cx);
    let picker = modal::<BranchPicker>(&workbench, cx).expect("the branch picker shows");
    let ids = picker.read_with(cx, |picker, _| picker.delegate.ids());
    let local = ids.iter().position(|id| id == "refs/heads/origin/feat");
    let row = picker.read_with(cx, |picker, _| picker.row_bounds(local.unwrap()));
    let row = row.expect("drawn");
    // Near its start: a row is only as wide as its text.
    let at = row.origin + gpui::point(gpui::px(4.), row.size.height / 2.);
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert_eq!(head(&work), "refs/heads/origin/feat");

    // A local branch of the remote's short name made after the listing
    // isn't taken for the remote one.
    git(&work, &["switch", "-q", "main"]);
    git(&work, &["branch", "-q", "-D", "feat", "origin/feat"]);
    run(&workbench, GitCommand::SwitchBranch, cx);
    let picker = modal::<BranchPicker>(&workbench, cx).expect("the branch picker shows");
    let names: Vec<String> = rows(&picker, cx)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, [CREATE_ROW, "main", "origin/feat"]);
    git(&work, &["branch", "-q", "origin/feat"]);
    cx.simulate_keystrokes("ctrl-n ctrl-n enter");
    draw(cx);
    assert_eq!(head(&work), "refs/heads/feat");
}

/// A remote whose name holds a slash.
#[gpui::test]
fn remote_name_with_a_slash(cx: &mut TestAppContext) {
    let dir = fixture();
    let work = dir.path().join("work");
    let origin = dir.path().join("origin.git");
    git(
        &work,
        &["remote", "add", "team/origin", origin.to_str().unwrap()],
    );
    git(&work, &["fetch", "-q", "team/origin"]);
    git(&work, &["branch", "-q", "-D", "feat"]);
    init(cx);
    let (workbench, mut cx) = window(cx, &work);
    let cx = &mut cx;

    run(&workbench, GitCommand::SwitchBranch, cx);
    cx.simulate_input("team/");
    draw(cx);
    let picker = modal::<BranchPicker>(&workbench, cx).expect("the branch picker shows");
    let names: Vec<String> = rows(&picker, cx)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(names, [CREATE_ROW, "team/origin/feat"]);
    cx.simulate_keystrokes("ctrl-n enter");
    draw(cx);
    assert_eq!(git(&work, &["symbolic-ref", "HEAD"]), "refs/heads/feat");
    let upstream = git(
        &work,
        &["rev-parse", "--symbolic-full-name", "feat@{upstream}"],
    );
    assert_eq!(upstream, "refs/remotes/team/origin/feat");
}

#[path = "git_branch_marks_tests.rs"]
mod marks;

#[path = "git_branch_merge_marks_tests.rs"]
mod merge_marks;
