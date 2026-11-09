//! Stashing and popping or applying stashes, on repositories built with the
//! `git` CLI in temporary directories: the message's picker, the stash
//! picker, the Changes panel and open files refreshed after each, and the
//! toasts when there's nothing to do or git fails.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use fe_gpui::{EditorSettings, EditorTheme, EditorView, set_vim_enabled};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::Picker;

use super::{MessagePicker, NO_STASHES, NOTHING_TO_SAVE, STASH_CHANGED, STASH_PICKER, StashPicker};
use crate::{
    Launch, Workbench,
    workbench::{git_commands::GitCommand, toast::Level},
};

/// Runs `git` in `dir`, with the developer's own config shut out; its
/// output, trimmed.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
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

/// A repository on main whose `a.txt` was committed as `one`, then
/// changed to `two`, beside an untracked `new.txt`. Its own config names
/// the committer, as the workbench's stashes read the developer's config
/// too.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    for (key, value) in [
        ("user.name", "t"),
        ("user.email", "t@t"),
        ("commit.gpgsign", "false"),
    ] {
        git(root, &["config", key, value]);
    }
    fs::write(root.join("a.txt"), "one\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("a.txt"), "two\n").unwrap();
    fs::write(root.join("new.txt"), "new\n").unwrap();
    dir
}

/// A window of a workbench rooted at `root`, focused, with vim on and
/// `a.txt` open.
fn window(cx: &mut TestAppContext, root: &Path) -> (Entity<Workbench>, VisualTestContext) {
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
    let cx = &mut vcx;
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = root.join("a.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
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

/// The stash picker of type `D` showing, if one is.
fn modal<D: crate::PickerDelegate>(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<Picker<D>>> {
    workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        if layer.modal() != Some(STASH_PICKER) {
            return None;
        }
        layer.modal_view()?.clone().downcast().ok()
    })
}

/// The paths the Changes panel lists, from the root.
fn rows(workbench: &Entity<Workbench>, root: &Path, cx: &mut VisualTestContext) -> Vec<PathBuf> {
    let changes = workbench.read_with(cx, |workbench, cx| {
        workbench.dock.read(cx).changes().clone()
    });
    let mut paths = changes.read_with(cx, |changes, _| changes.paths());
    paths.sort();
    (paths.iter())
        .map(|path| path.strip_prefix(root).unwrap_or(path).to_owned())
        .collect()
}

fn paths(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

/// The shown toasts' levels, messages and details, emptied.
fn toasts(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(Level, String, Option<String>)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    let shown = layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let toast = &entry.toast;
                (toast.level, toast.message.clone(), toast.detail.clone())
            })
            .collect()
    });
    cx.update(|_, cx| layer.update(cx, |layer, cx| layer.dismiss_all(cx)));
    shown
}

/// The stashes, as `git stash list` prints them.
fn stashes(root: &Path) -> Vec<String> {
    let list = git(root, &["stash", "list", "--format=%gd: %gs"]);
    list.lines().map(str::to_owned).collect()
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string())
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor is active")
}

fn picked_rows(picker: &Entity<Picker<StashPicker>>, cx: &mut VisualTestContext) -> Vec<String> {
    picker.read_with(cx, |picker, _| picker.delegate.labels())
}

#[gpui::test]
fn commands(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let (workbench, mut cx) = window(cx, &root);
    let cx = &mut cx;
    let editor = active(&workbench, cx);
    assert_eq!(rows(&workbench, &root, cx), paths(&["a.txt", "new.txt"]));

    // git.stash asks for a message, then stashes the tracked change.
    run(&workbench, GitCommand::Stash, cx);
    let picker = modal::<MessagePicker>(&workbench, cx).expect("the message's picker shows");
    let label =
        |cx: &mut VisualTestContext| picker.read_with(cx, |picker, _| picker.delegate.label());
    assert_eq!(label(cx), "Stash local changes");
    cx.simulate_input("wip");
    draw(cx);
    assert_eq!(label(cx), "Stash local changes, as 'wip'");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(modal::<MessagePicker>(&workbench, cx).is_none());
    assert!(cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)));
    assert_eq!(stashes(&root), ["stash@{0}: On main: wip"]);
    assert_eq!(rows(&workbench, &root, cx), paths(&["new.txt"]));
    // The open file is reloaded from disk.
    assert_eq!(text(&editor, cx), "one\n");
    assert_eq!(toasts(&workbench, cx), []);

    // git.stash_apply shows the stashes and applies the one picked,
    // keeping it.
    run(&workbench, GitCommand::StashApply, cx);
    let picker = modal::<StashPicker>(&workbench, cx).expect("the stash picker shows");
    assert_eq!(picked_rows(&picker, cx), ["stash@{0}: wip"]);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(modal::<StashPicker>(&workbench, cx).is_none());
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "two\n");
    assert_eq!(text(&editor, cx), "two\n");
    assert_eq!(stashes(&root), ["stash@{0}: On main: wip"]);
    assert_eq!(rows(&workbench, &root, cx), paths(&["a.txt", "new.txt"]));

    // Applying it again over the same change: git refuses, in a toast.
    fs::write(root.join("a.txt"), "three\n").unwrap();
    run(&workbench, GitCommand::StashApply, cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    let shown = toasts(&workbench, cx);
    let [(Level::Error, message, Some(detail))] = shown.as_slice() else {
        panic!("an error toast: {shown:?}");
    };
    assert_eq!(message, "Can't apply stash@{0}");
    assert!(detail.contains("a.txt"), "{detail}");
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "three\n");

    // A second stash; git.stash_pop lists it first, and pops the older
    // one picked, typed to filter.
    git(&root, &["stash", "push", "-q", "-m", "later"]);
    fs::write(root.join("a.txt"), "two\n").unwrap();
    git(&root, &["stash", "push", "-q", "-m", "latest"]);
    draw(cx);
    run(&workbench, GitCommand::StashPop, cx);
    let picker = modal::<StashPicker>(&workbench, cx).expect("the stash picker shows");
    assert_eq!(
        picked_rows(&picker, cx),
        ["stash@{0}: latest", "stash@{1}: later", "stash@{2}: wip"]
    );
    cx.simulate_input("wip");
    draw(cx);
    assert_eq!(picked_rows(&picker, cx), ["stash@{2}: wip"]);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(
        stashes(&root),
        ["stash@{0}: On main: latest", "stash@{1}: On main: later"]
    );
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "two\n");
    assert_eq!(rows(&workbench, &root, cx), paths(&["a.txt", "new.txt"]));
    assert_eq!(toasts(&workbench, cx), []);

    // A pop that conflicts: a warning naming the file, the entry kept.
    git(&root, &["commit", "-q", "-am", "two"]);
    run(&workbench, GitCommand::StashPop, cx);
    cx.simulate_keystrokes("ctrl-n enter");
    draw(cx);
    let shown = toasts(&workbench, cx);
    let [(Level::Warning, message, Some(detail))] = shown.as_slice() else {
        panic!("a warning toast: {shown:?}");
    };
    assert_eq!(message, "Conflicts applying stash@{1}");
    assert!(detail.contains("Merge conflict in a.txt"), "{detail}");
    assert!(detail.contains("The stash entry is kept"), "{detail}");
    assert_eq!(stashes(&root).len(), 2);
    assert_eq!(rows(&workbench, &root, cx), paths(&["a.txt", "new.txt"]));
    assert!(text(&editor, cx).contains("<<<<<<<"));

    // Nothing to stash: said in a toast, no entry made.
    git(&root, &["checkout", "-q", "HEAD", "--", "a.txt"]);
    fs::remove_file(root.join("new.txt")).unwrap();
    run(&workbench, GitCommand::Stash, cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    let nothing = (Level::Info, NOTHING_TO_SAVE.to_owned(), None);
    assert_eq!(toasts(&workbench, cx), [nothing]);
    assert_eq!(stashes(&root).len(), 2);

    // No stashes: said in a toast, no picker.
    git(&root, &["stash", "clear"]);
    run(&workbench, GitCommand::StashPop, cx);
    assert!(modal::<StashPicker>(&workbench, cx).is_none());
    let none = (Level::Info, NO_STASHES.to_owned(), None);
    assert_eq!(toasts(&workbench, cx), [none]);
}

#[gpui::test]
fn include_untracked(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let (workbench, mut cx) = window(cx, &root);
    let cx = &mut cx;

    run(&workbench, GitCommand::StashIncludeUntracked, cx);
    let picker = modal::<MessagePicker>(&workbench, cx).expect("the message's picker shows");
    let label = picker.read_with(cx, |picker, _| picker.delegate.label());
    assert_eq!(label, "Stash local changes, untracked files included");
    // Escape (vim's normal mode, then the picker) stashes nothing.
    cx.simulate_keystrokes("escape escape");
    draw(cx);
    assert!(modal::<MessagePicker>(&workbench, cx).is_none());
    assert_eq!(stashes(&root).len(), 0);

    // With no message, git's.
    run(&workbench, GitCommand::StashIncludeUntracked, cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    let head = git(&root, &["rev-parse", "--short", "HEAD"]);
    assert_eq!(
        stashes(&root),
        [format!("stash@{{0}}: WIP on main: {head} init")]
    );
    assert!(!root.join("new.txt").exists());
    assert_eq!(rows(&workbench, &root, cx), paths(&[]));

    run(&workbench, GitCommand::StashPop, cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(stashes(&root).len(), 0);
    assert_eq!(fs::read_to_string(root.join("new.txt")).unwrap(), "new\n");
    assert_eq!(rows(&workbench, &root, cx), paths(&["a.txt", "new.txt"]));
}

/// A stash pushed while the picker shows moves the entry listed: the pick
/// does nothing, and says so.
#[gpui::test]
fn stash_moved(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dunce::canonicalize(dir.path()).unwrap();
    git(&root, &["stash", "push", "-q", "-m", "wip"]);
    let (workbench, mut cx) = window(cx, &root);
    let cx = &mut cx;

    run(&workbench, GitCommand::StashPop, cx);
    let picker = modal::<StashPicker>(&workbench, cx).expect("the stash picker shows");
    assert_eq!(picked_rows(&picker, cx), ["stash@{0}: wip"]);
    fs::write(root.join("a.txt"), "three\n").unwrap();
    git(&root, &["stash", "push", "-q", "-m", "other"]);
    cx.simulate_keystrokes("enter");
    draw(cx);
    let moved = (Level::Info, STASH_CHANGED.to_owned(), None);
    assert_eq!(toasts(&workbench, cx), [moved]);
    assert_eq!(
        stashes(&root),
        ["stash@{0}: On main: other", "stash@{1}: On main: wip"]
    );
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "one\n");
}

#[gpui::test]
fn outside_a_repository(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;
    for command in [GitCommand::Stash, GitCommand::StashPop] {
        run(&workbench, command, cx);
        assert!(modal::<MessagePicker>(&workbench, cx).is_none());
        assert!(modal::<StashPicker>(&workbench, cx).is_none());
        let shown = toasts(&workbench, cx);
        let messages: Vec<&str> = shown
            .iter()
            .map(|(_, message, _)| message.as_str())
            .collect();
        assert_eq!(messages, ["Not in a git repository"], "{command:?}");
    }
}

/// Runs `command` without drawing, so a listing can stay pending.
fn start(workbench: &Entity<Workbench>, command: GitCommand, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.run_git_command(command, window, cx)
        })
    });
}

/// Pop then Apply listed at once: only the latest, Apply's, shows, in
/// whichever order the listings finish.
#[gpui::test]
fn overlapping_listings(cx: &mut TestAppContext) {
    for release_first_started in [true, false] {
        let dir = fixture();
        let root = dunce::canonicalize(dir.path()).unwrap();
        git(&root, &["stash", "push", "-q", "-m", "wip"]);
        let (workbench, mut cx) = window(cx, &root);
        let cx = &mut cx;

        cx.update(|_, cx| cx.default_global::<super::ListingHolds>().holding = true);
        start(&workbench, GitCommand::StashPop, cx);
        start(&workbench, GitCommand::StashApply, cx);
        cx.run_until_parked();
        assert!(modal::<StashPicker>(&workbench, cx).is_none());
        let mut held =
            cx.update(|_, cx| std::mem::take(&mut cx.global_mut::<super::ListingHolds>().held));
        assert_eq!(held.len(), 2);
        cx.update(|_, cx| cx.global_mut::<super::ListingHolds>().holding = false);
        if !release_first_started {
            held.reverse();
        }
        for release in held {
            release.send(()).ok();
            cx.run_until_parked();
        }
        draw(cx);
        let picker = modal::<StashPicker>(&workbench, cx).expect("a picker shows");
        assert!(picker.read_with(cx, |picker, _| picker.delegate.verb_is_apply()));
        cx.simulate_keystrokes("enter");
        draw(cx);
        // Applied, not popped: the entry is kept.
        assert_eq!(stashes(&root), ["stash@{0}: On main: wip"]);
    }
}

/// A commit stored twice is two rows, each picked on its own.
#[gpui::test]
fn duplicate_commits(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dunce::canonicalize(dir.path()).unwrap();
    git(&root, &["stash", "push", "-q", "-m", "wip"]);
    let id = git(&root, &["rev-parse", "stash@{0}"]);
    fs::write(root.join("a.txt"), "three\n").unwrap();
    git(&root, &["stash", "push", "-q", "-m", "other"]);
    git(&root, &["stash", "store", "-m", "again", &id]);
    let (workbench, mut cx) = window(cx, &root);
    let cx = &mut cx;

    run(&workbench, GitCommand::StashPop, cx);
    let picker = modal::<StashPicker>(&workbench, cx).expect("the stash picker shows");
    let ids = picker.read_with(cx, |picker, _| {
        use crate::PickerDelegate as _;
        (0..picker.delegate.match_count())
            .map(|ix| picker.delegate.match_id(ix))
            .collect::<Vec<_>>()
    });
    assert_eq!(ids.len(), 3);
    assert_ne!(ids[0], ids[2]);
    // Picking the older duplicate pops stash@{2}, not the first match.
    picker.update_in(cx, |picker, window, cx| picker.confirm(2, window, cx));
    draw(cx);
    assert_eq!(
        stashes(&root),
        ["stash@{0}: again", "stash@{1}: On main: other"]
    );
}
