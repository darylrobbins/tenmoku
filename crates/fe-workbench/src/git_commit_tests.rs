//! Committing from the Changes panel's message box: the hints, Amend, the
//! stage-all question, a failing hook, and vim's keys and ex commands in
//! the box, on repositories built with the `git` CLI in temporary
//! directories.

use std::{fs, os::unix::fs::PermissionsExt as _, path::Path, process::Command};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_editor::decorations::Tint;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::STAGE_ALL_AND_COMMIT;
use crate::{
    GitPanel, Launch, Workbench,
    git_panel::{COMMIT_COMMAND, COMMIT_KEY, HINT_ROOM, set_box_text},
};

/// Runs `git` in `dir`, with the developer's own config shut out; its
/// output.
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

/// A repository whose `a.rs` was committed as `init`, then changed and
/// staged. Its own config names the committer and its hooks, as the
/// workbench's commits read the developer's config too.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    for (key, value) in [
        ("user.name", "t"),
        ("user.email", "t@t"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", ".git/hooks"),
    ] {
        git(root, &["config", key, value]);
    }
    fs::write(root.join("a.rs"), "head\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("a.rs"), "staged\n").unwrap();
    git(root, &["add", "a.rs"]);
    dir
}

/// Has the repository at `root` run `body` as its `name` hook.
fn hook(root: &Path, name: &str, body: &str) {
    let hooks = root.join(".git/hooks");
    fs::create_dir_all(&hooks).unwrap();
    let path = hooks.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A workbench for `root`, in a headless window, focused as the app
/// focuses it, showing the Changes panel and its box.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (
    Entity<Workbench>,
    Entity<GitPanel>,
    Entity<EditorView>,
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
    draw(cx);
    dispatch("git.show_changes", cx);
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).changes().clone()
    });
    let commit_box = panel.read_with(cx, |panel, _| panel.commit_box().cloned());
    let commit_box = commit_box.expect("the panel's drawn with its box");
    (workbench, panel, commit_box, cx)
}

/// Lets background work land, then draws a frame.
fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs workbench command `id` as an unbound key would.
fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Runs [`COMMIT_COMMAND`] in the box, as its key does.
fn commit(commit_box: &Entity<EditorView>, cx: &mut VisualTestContext) {
    let run = RunCommand(Invocation::of(COMMIT_COMMAND).unwrap());
    cx.update(|window, cx| {
        commit_box
            .focus_handle(cx)
            .dispatch_action(&run, window, cx)
    });
    draw(cx);
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

/// Types `:{line}` and Enter.
fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    draw(cx);
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

fn focus_box(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) {
    panel.update_in(cx, |panel, window, cx| panel.focus_commit_box(window, cx));
    draw(cx);
}

/// Checks or unchecks Amend, as its checkbox does.
fn check_amend(panel: &Entity<GitPanel>, amend: bool, cx: &mut VisualTestContext) {
    panel.update_in(cx, |panel, window, cx| panel.set_amend(amend, window, cx));
    draw(cx);
}

/// The box's height as drawn, in its rows.
fn box_height(commit_box: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    let line_height = commit_box.read_with(cx, |editor, _| {
        let theme = editor.theme();
        (theme.font_size * theme.line_height).round()
    });
    let bounds = cx.debug_bounds("commit-box").expect("the box is drawn");
    bounds.size.height / line_height
}

fn text(commit_box: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    commit_box.read_with(cx, |editor, _| editor.model().text().to_string())
}

fn set_text(commit_box: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    cx.update(|_, cx| set_box_text(commit_box, text, cx));
    draw(cx);
}

fn focused(commit_box: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| commit_box.focus_handle(cx).is_focused(window))
}

fn mode(commit_box: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    commit_box.read_with(cx, |editor, _| editor.vim_mode())
}

fn vim_message(commit_box: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    commit_box.read_with(cx, |editor, _| {
        Some(editor.vim_state()?.message()?.to_owned())
    })
}

/// HEAD's subject and body, as `git log` shows them.
fn head_message(root: &Path) -> String {
    git(root, &["log", "-1", "--format=%B"])
}

fn commit_count(root: &Path) -> usize {
    git(root, &["rev-list", "--count", "HEAD"]).parse().unwrap()
}

/// Each group's title and count, as the panel shows them.
fn groups(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> Vec<(&'static str, usize)> {
    panel.read_with(cx, |panel, _| {
        (panel.groups().into_iter())
            .map(|(title, count, _)| (title, count))
            .collect()
    })
}

#[gpui::test]
fn commits(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, panel, commit_box, cx) = open(cx, root);
    assert_eq!(groups(&panel, cx), [("Staged Changes", 1), ("Changes", 0)]);

    // Empty, the box names the branch, and the button's off.
    let placeholder = format!("Message ({COMMIT_KEY} to commit on \"main\")");
    let hints = panel.read_with(cx, |panel, cx| panel.commit_hints(cx));
    assert_eq!(hints, [(0, placeholder, Tint::Muted)]);
    let button = panel.read_with(cx, |panel, cx| panel.commit_button_state(cx));
    assert_eq!(button, Some(("Commit", false)));

    focus_box(&panel, cx);
    cx.simulate_input("Fix it");
    draw(cx);
    assert_eq!(text(&commit_box, cx), "Fix it");
    let button = panel.read_with(cx, |panel, cx| panel.commit_button_state(cx));
    assert_eq!(button, Some(("Commit", true)));

    commit(&commit_box, cx);
    assert_eq!(head_message(root), "Fix it");
    assert_eq!(git(root, &["show", "HEAD:a.rs"]), "staged");
    assert_eq!(commit_count(root), 2);
    assert_eq!(text(&commit_box, cx), "");
    assert_eq!(groups(&panel, cx), []);
    let message = panel.read_with(cx, |panel, _| panel.message_text());
    assert_eq!(message.as_deref(), Some("No changes"));

    // The subject's length shows over 50, muted, and warns over 72.
    set_text(&commit_box, &"s".repeat(55), cx);
    let hints = panel.read_with(cx, |panel, cx| panel.commit_hints(cx));
    assert_eq!(hints, [(0, "55/50".to_owned(), Tint::Muted)]);
    set_text(&commit_box, &"s".repeat(80), cx);
    let hints = panel.read_with(cx, |panel, cx| panel.commit_hints(cx));
    assert_eq!(hints, [(0, "80/50".to_owned(), Tint::Warning)]);
    set_text(&commit_box, &"s".repeat(50), cx);
    let hints = panel.read_with(cx, |panel, cx| panel.commit_hints(cx));
    assert_eq!(hints, []);
    // A body row over 72, after the first blank row, warns.
    let message = format!(
        "Subject\n{}\n\n{}\n{}",
        "x".repeat(80),
        "b".repeat(73),
        "c".repeat(72)
    );
    set_text(&commit_box, &message, cx);
    let hints = panel.read_with(cx, |panel, cx| panel.commit_hints(cx));
    assert_eq!(hints, [(3, "73/72".to_owned(), Tint::Warning)]);

    // Soft wrap leaves room after a row for its length hint.
    let reserve = commit_box.read_with(cx, |editor, _| editor.wrap_reserve());
    assert_eq!(reserve, HINT_ROOM);

    // The box grows with its text, to at most 10 rows.
    set_text(&commit_box, "One row", cx);
    let one = box_height(&commit_box, cx);
    assert!((1.5..1.6).contains(&one), "{one}");
    let long = vec!["row"; 30].join("\n");
    set_text(&commit_box, &long, cx);
    let most = box_height(&commit_box, cx);
    assert!((10.0..10.1).contains(&most), "{most}");

    // The box is no tab, and its text no file.
    let tabs = workbench.read_with(cx, |workbench, cx| workbench.open_editors(cx));
    assert!(!tabs.contains(&commit_box));
    let path = commit_box.read_with(cx, |editor, _| editor.model().path().map(Path::to_owned));
    assert_eq!(path, None);
}

#[gpui::test]
fn amend(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    git(root, &["commit", "-q", "-m", "Second\n\nIts body"]);
    fs::write(root.join("a.rs"), "amended\n").unwrap();
    git(root, &["add", "a.rs"]);
    let parent = git(root, &["rev-parse", "HEAD~1"]);
    let (_workbench, panel, commit_box, cx) = open(cx, root);

    // Checking Amend into an empty box fills it with HEAD's message, and
    // unchecking it while that's untouched empties it again.
    check_amend(&panel, true, cx);
    draw(cx);
    assert_eq!(text(&commit_box, cx), "Second\n\nIts body");
    let button = panel.read_with(cx, |panel, cx| panel.commit_button_state(cx));
    assert_eq!(button, Some(("Amend", true)));
    check_amend(&panel, false, cx);
    draw(cx);
    assert_eq!(text(&commit_box, cx), "");
    assert!(!panel.read_with(cx, |panel, _| panel.amend_checked()));

    // Edited, it stays as it is.
    check_amend(&panel, true, cx);
    draw(cx);
    set_text(&commit_box, "Second, edited", cx);
    check_amend(&panel, false, cx);
    draw(cx);
    assert_eq!(text(&commit_box, cx), "Second, edited");

    // A box with a message isn't filled.
    check_amend(&panel, true, cx);
    draw(cx);
    assert_eq!(text(&commit_box, cx), "Second, edited");

    // Committing amends HEAD: the same parent, the new message and tree.
    commit(&commit_box, cx);
    assert_eq!(git(root, &["rev-parse", "HEAD~1"]), parent);
    assert_eq!(head_message(root), "Second, edited");
    assert_eq!(git(root, &["show", "HEAD:a.rs"]), "amended");
    assert_eq!(commit_count(root), 2);
    assert_eq!(text(&commit_box, cx), "");
    assert!(!panel.read_with(cx, |panel, _| panel.amend_checked()));
}

#[gpui::test]
fn failures(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    git(root, &["commit", "-q", "-m", "staged"]);
    fs::write(root.join("a.rs"), "changed\n").unwrap();
    fs::write(root.join("b.rs"), "new\n").unwrap();
    let (_workbench, panel, commit_box, cx) = open(cx, root);

    // With nothing staged, it asks whether to stage everything.
    set_text(&commit_box, "Everything", cx);
    commit(&commit_box, cx);
    let (question, detail) = cx.pending_prompt().expect("a question");
    assert_eq!(question, "There are no staged changes to commit.");
    assert_eq!(
        detail,
        "Would you like to stage all your changes and commit them directly?"
    );
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert_eq!(commit_count(root), 2);
    assert_eq!(text(&commit_box, cx), "Everything");

    commit(&commit_box, cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer(STAGE_ALL_AND_COMMIT);
    draw(cx);
    assert_eq!(commit_count(root), 3);
    assert_eq!(head_message(root), "Everything");
    assert_eq!(git(root, &["show", "HEAD:a.rs"]), "changed");
    assert_eq!(git(root, &["show", "HEAD:b.rs"]), "new");
    assert_eq!(git(root, &["status", "--porcelain"]), "");
    assert_eq!(text(&commit_box, cx), "");

    // A failing hook shows what it printed, and the message stays.
    hook(root, "pre-commit", "echo 'lint failed'\nexit 1");
    fs::write(root.join("a.rs"), "linted\n").unwrap();
    git(root, &["add", "a.rs"]);
    set_text(&commit_box, "Lint me", cx);
    commit(&commit_box, cx);
    let (message, detail) = cx.pending_prompt().expect("a warning");
    assert_eq!(message, "Git: pre-commit hook failed");
    assert!(detail.contains("lint failed"), "{detail}");
    cx.simulate_prompt_answer("OK");
    draw(cx);
    assert_eq!(commit_count(root), 3);
    assert_eq!(text(&commit_box, cx), "Lint me");
    let button = panel.read_with(cx, |panel, cx| panel.commit_button_state(cx));
    assert_eq!(button, Some(("Commit", true)));

    // An empty message is asked for in the placeholder.
    set_text(&commit_box, "", cx);
    commit(&commit_box, cx);
    assert!(!cx.has_pending_prompt());
    let hints = panel.read_with(cx, |panel, cx| panel.commit_hints(cx));
    assert_eq!(
        hints,
        [(0, "Enter a commit message".to_owned(), Tint::Warning)]
    );
    set_text(&commit_box, "Typed", cx);
    set_text(&commit_box, "", cx);
    let hints = panel.read_with(cx, |panel, cx| panel.commit_hints(cx));
    let placeholder = format!("Message ({COMMIT_KEY} to commit on \"main\")");
    assert_eq!(hints, [(0, placeholder, Tint::Muted)]);

    // One of only blanks or comments is asked for under the box, until
    // the box is edited.
    for message in ["  \n ", "# Only a comment"] {
        set_text(&commit_box, message, cx);
        commit(&commit_box, cx);
        assert!(!cx.has_pending_prompt(), "{message:?}");
        assert_eq!(commit_count(root), 3, "{message:?}");
        let warning = panel.read_with(cx, |panel, cx| panel.commit_warning(cx));
        assert_eq!(warning, Some("Enter a commit message"), "{message:?}");
        let hints = panel.read_with(cx, |panel, cx| panel.commit_hints(cx));
        assert_eq!(hints, [], "{message:?}");
        set_text(&commit_box, &format!("{message}."), cx);
        let warning = panel.read_with(cx, |panel, cx| panel.commit_warning(cx));
        assert_eq!(warning, None, "{message:?}");
    }
}

#[gpui::test]
fn vim_box(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, panel, commit_box, cx) = open(cx, root);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let path = root.join("a.rs");
    workbench.update_in(cx, |workbench, window, cx| {
        (workbench.active).update(cx, |pane, cx| pane.open(path, window, cx))
    });
    draw(cx);
    let tab = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let tab = tab.expect("a.rs's tab");
    let tab_focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| tab.focus_handle(cx).is_focused(window))
    };

    // `git.commit` focuses the empty box in insert mode.
    dispatch("git.commit", cx);
    assert!(focused(&commit_box, cx));
    assert_eq!(mode(&commit_box, cx), Some("insert"));
    cx.simulate_input("First");
    keys("escape", cx);
    assert_eq!(mode(&commit_box, cx), Some("normal"));

    // `:w` keeps it, `:q` won't leave it, and `:wq` commits.
    ex("w", cx);
    assert_eq!(
        vim_message(&commit_box, cx).as_deref(),
        Some("Message kept; :wq commits")
    );
    assert_eq!(commit_count(root), 1);
    ex("q", cx);
    let e37 = "E37: No write since last change (add ! to override)";
    assert_eq!(vim_message(&commit_box, cx).as_deref(), Some(e37));
    assert!(focused(&commit_box, cx));
    assert!(!panel.read_with(cx, |panel, _| panel.commit_line_open()));
    ex("wq", cx);
    assert_eq!(head_message(root), "First");
    assert_eq!(commit_count(root), 2);
    assert_eq!(text(&commit_box, cx), "");

    // `ZZ` commits.
    fs::write(root.join("a.rs"), "second\n").unwrap();
    git(root, &["add", "a.rs"]);
    dispatch("git.commit", cx);
    cx.simulate_input("Second");
    keys("escape Z Z", cx);
    assert_eq!(head_message(root), "Second");
    assert_eq!(commit_count(root), 3);

    // `:q!` and `ZQ` empty the box and go back to the editor.
    for abort in ["q!", "ZQ"] {
        dispatch("git.commit", cx);
        assert_eq!(mode(&commit_box, cx), Some("insert"));
        cx.simulate_input("Dropped");
        keys("escape", cx);
        match abort {
            "ZQ" => keys("Z Q", cx),
            line => ex(line, cx),
        }
        assert_eq!(text(&commit_box, cx), "", "{abort}");
        assert!(tab_focused(cx), "{abort}");
        // Focused again, the empty box is in insert mode.
        cx.update(|window, cx| window.focus(&commit_box.focus_handle(cx), cx));
        draw(cx);
        assert_eq!(mode(&commit_box, cx), Some("insert"), "{abort}");
        keys("escape", cx);
        cx.update(|window, cx| window.focus(&tab.focus_handle(cx), cx));
        draw(cx);
    }
    assert_eq!(commit_count(root), 3);

    // `:q` on an empty box goes back to the editor too; `:w` leaves it in
    // normal mode.
    dispatch("git.commit", cx);
    keys("escape", cx);
    ex("w", cx);
    assert!(focused(&commit_box, cx));
    assert_eq!(mode(&commit_box, cx), Some("normal"));
    ex("q", cx);
    assert!(tab_focused(cx));
    cx.update(|window, cx| window.focus(&commit_box.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(mode(&commit_box, cx), Some("insert"));
    cx.update(|window, cx| window.focus(&tab.focus_handle(cx), cx));
    draw(cx);

    // A draft left in insert mode opens again in normal mode.
    dispatch("git.commit", cx);
    cx.simulate_input("Draft");
    cx.update(|window, cx| window.focus(&tab.focus_handle(cx), cx));
    draw(cx);
    assert_eq!(mode(&commit_box, cx), Some("insert"));
    dispatch("git.commit", cx);
    assert!(focused(&commit_box, cx));
    assert_eq!(mode(&commit_box, cx), Some("normal"));
    keys("Z Q", cx);
    assert_eq!(text(&commit_box, cx), "");

    // `cc` in the panel focuses the box, `ca` with Amend checked.
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    draw(cx);
    keys("c c", cx);
    assert!(focused(&commit_box, cx));
    assert!(!panel.read_with(cx, |panel, _| panel.amend_checked()));
    keys("escape", cx);
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    draw(cx);
    keys("c a", cx);
    assert!(focused(&commit_box, cx));
    assert!(panel.read_with(cx, |panel, _| panel.amend_checked()));
    assert_eq!(text(&commit_box, cx), "Second");
    // Filled with HEAD's message, it's in normal mode.
    assert_eq!(mode(&commit_box, cx), Some("normal"));
}

#[gpui::test]
fn edits_while_committing(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (_workbench, panel, commit_box, cx) = open(cx, root);
    cx.update(|_, cx| Workbench::hold_git_work(cx));

    // What's typed while the commit runs stays once it's made.
    set_text(&commit_box, "Slow", cx);
    check_amend(&panel, true, cx);
    commit(&commit_box, cx);
    let button = panel.read_with(cx, |panel, cx| panel.commit_button_state(cx));
    assert_eq!(button, Some(("Committing…", false)));
    set_text(&commit_box, "Next", cx);
    assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1);
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(head_message(root), "Slow");
    assert_eq!(text(&commit_box, cx), "Next");
    assert!(!panel.read_with(cx, |panel, _| panel.amend_checked()));
    let button = panel.read_with(cx, |panel, cx| panel.commit_button_state(cx));
    assert_eq!(button, Some(("Commit", true)));

    // An empty message replaced while it's refused isn't asked for.
    set_text(&commit_box, "  ", cx);
    commit(&commit_box, cx);
    set_text(&commit_box, "Valid", cx);
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    let warning = panel.read_with(cx, |panel, cx| panel.commit_warning(cx));
    assert_eq!(warning, None);
    assert_eq!(text(&commit_box, cx), "Valid");
}

#[gpui::test]
fn stage_all_commits_what_was_submitted(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    git(root, &["commit", "-q", "-m", "staged"]);
    fs::write(root.join("a.rs"), "changed\n").unwrap();
    let (_workbench, _panel, commit_box, cx) = open(cx, root);
    cx.update(|_, cx| Workbench::hold_git_work(cx));

    // Edited before the refusal lands, the box keeps its new draft, and
    // staging everything commits the message submitted.
    set_text(&commit_box, "Submitted", cx);
    commit(&commit_box, cx);
    set_text(&commit_box, "Later", cx);
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    let (question, _) = cx.pending_prompt().expect("a question");
    assert_eq!(question, "There are no staged changes to commit.");
    cx.simulate_prompt_answer(STAGE_ALL_AND_COMMIT);
    draw(cx);
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(head_message(root), "Submitted");
    assert_eq!(git(root, &["show", "HEAD:a.rs"]), "changed");
    assert_eq!(commit_count(root), 3);
    assert_eq!(text(&commit_box, cx), "Later");
}

#[gpui::test]
fn stage_all_stays_in_its_root(cx: &mut TestAppContext) {
    let first = fixture();
    let a = first.path();
    git(a, &["commit", "-q", "-m", "staged"]);
    fs::write(a.join("a.rs"), "changed in a\n").unwrap();
    let second = fixture();
    let b = second.path();
    let (workbench, _panel, commit_box, cx) = open(cx, a);
    let set_root = |root: &Path, cx: &mut VisualTestContext| {
        let root = root.to_owned();
        workbench.update_in(cx, |workbench, window, cx| {
            workbench.set_root(root, window, cx)
        });
        draw(cx);
    };

    // The root changing while the question's asked: staging everything
    // does nothing.
    set_text(&commit_box, "In a", cx);
    commit(&commit_box, cx);
    assert!(cx.has_pending_prompt());
    set_root(b, cx);
    cx.simulate_prompt_answer(STAGE_ALL_AND_COMMIT);
    draw(cx);
    assert_eq!((commit_count(a), commit_count(b)), (2, 1));
    assert_eq!(git(a, &["status", "--porcelain"]), "M a.rs");

    // Nor is it asked once the root's changed before the refusal lands.
    set_root(a, cx);
    let commit_box = workbench.read_with(cx, |workbench, cx| {
        let changes = workbench.dock().read(cx).changes().clone();
        changes.read(cx).commit_box().cloned()
    });
    let commit_box = commit_box.expect("the new panel's box");
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    set_text(&commit_box, "In a", cx);
    commit(&commit_box, cx);
    set_root(b, cx);
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!((commit_count(a), commit_count(b)), (2, 1));
}

#[gpui::test]
fn commits_wait_for_earlier_staging(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    fs::write(root.join("b.rs"), "new\n").unwrap();
    let (workbench, _panel, commit_box, cx) = open(cx, root);
    cx.update(|_, cx| Workbench::hold_git_work(cx));

    // Staging b.rs is held; the commit asked after it waits its turn.
    let b = root.join("b.rs");
    workbench.update(cx, |workbench, cx| workbench.stage_files(vec![b], true, cx));
    draw(cx);
    set_text(&commit_box, "With b", cx);
    commit(&commit_box, cx);
    assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1);
    assert_eq!(commit_count(root), 1);
    let button = panel_button(&workbench, cx);
    assert_eq!(button, Some(("Committing…", false)));

    // Once the staging's done, the commit starts, and commits its index.
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(
        git(root, &["diff", "--cached", "--name-only"]),
        "a.rs\nb.rs"
    );
    assert_eq!(commit_count(root), 1);
    assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1);
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(head_message(root), "With b");
    assert_eq!(git(root, &["show", "HEAD:b.rs"]), "new");
    assert_eq!(git(root, &["show", "HEAD:a.rs"]), "staged");
    assert_eq!(text(&commit_box, cx), "");
}

/// The Commit button's label and whether it's enabled, in `workbench`'s
/// Changes panel.
fn panel_button(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<(&'static str, bool)> {
    workbench.read_with(cx, |workbench, cx| {
        let changes = workbench.dock().read(cx).changes().clone();
        changes.read(cx).commit_button_state(cx)
    })
}
