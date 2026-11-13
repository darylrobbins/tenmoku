//! Changes and commit in B3 (`docs/design/design-system.md`, Changes and
//! commit): a checkbox on each row and header that stages and unstages,
//! status letters in the theme's `vcs.*` colours, the message box's
//! placeholder in the UI font and its 72-character counter, and the
//! "Commit 1 file" button with its key and its "and push", on repositories
//! built with the `git` CLI in temporary directories.

use std::{fs, path::Path, process::Command};

use fe_command::{CommandId, ContextFrame, ContextStack, Invocation, Value};
use fe_git::ChangeKind;
use fe_gpui::{EditorSettings, EditorTheme, EditorView, commands::RunCommand};
use fe_ui::{ActiveTheme as _, FamilySlot, FontFamilies};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{
    GitPanel, Launch, Workbench,
    git_panel::{self, Action, Group, LINE_LENGTH, set_box_text},
    workbench::git_commands::{GitCommand, Routed},
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
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned()
}

/// A repository with `a.rs`, `b.rs`, `c.rs` and `d.rs` committed, then
/// `a.rs` changed and staged, `b.rs` and `c.rs` changed, `d.rs` deleted
/// and `e.rs` new and untracked. Its own config names the committer, as
/// the workbench's commits read the developer's config too.
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
    for name in ["a.rs", "b.rs", "c.rs", "d.rs"] {
        fs::write(root.join(name), "head\n").unwrap();
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    for name in ["a.rs", "b.rs", "c.rs"] {
        fs::write(root.join(name), "changed\n").unwrap();
    }
    git(root, &["add", "a.rs"]);
    fs::remove_file(root.join("d.rs")).unwrap();
    fs::write(root.join("e.rs"), "new\n").unwrap();
    dir
}

/// Each file `git status` lists, as `XY path`.
fn status(root: &Path) -> Vec<String> {
    let status = git(root, &["status", "--porcelain"]);
    status.lines().map(str::to_owned).collect()
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

/// Each group's title, and its rows' letters and paths.
fn groups(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> Vec<(String, Vec<String>)> {
    panel.read_with(cx, |panel, _| {
        (panel.groups().into_iter())
            .map(|(title, _, rows)| {
                let rows = (rows.into_iter())
                    .map(|(letter, path)| format!("{letter} {path}"))
                    .collect();
                (title, rows)
            })
            .collect()
    })
}

/// Clicks file `ix`'s checkbox, or with `ix` `None` `group`'s header's,
/// as `action` (stage or unstage) names it.
fn click_checkbox(
    panel: &Entity<GitPanel>,
    ix: Option<usize>,
    group: Group,
    action: Action,
    cx: &mut VisualTestContext,
) {
    let selector = panel.read_with(cx, |panel, _| panel.button_selector(ix, group, action));
    let bounds = cx.debug_bounds(selector.leak()).expect("drawn");
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

fn set_text(commit_box: &Entity<EditorView>, text: &str, cx: &mut VisualTestContext) {
    cx.update(|_, cx| set_box_text(commit_box, text, cx));
    draw(cx);
}

/// Puts the box's caret at `offset`.
fn set_caret(commit_box: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) {
    commit_box.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(offset);
        cx.notify();
    });
    draw(cx);
}

/// The key the keymap binds to commit from the box, as it's printed.
fn commit_key(cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| {
        let keymap = fe_gpui::current_keymap(cx);
        let command = CommandId::parse(git_panel::COMMIT_COMMAND).unwrap();
        let stack: ContextStack = ["workbench", "git_panel", "editor"]
            .into_iter()
            .map(ContextFrame::new)
            .collect();
        let keys = fe_keymap::shortcuts(&keymap, &command, &Value::Null, &stack);
        fe_keymap::shortcut_label(keys.first().expect("a key commits from the box"))
    })
}

#[gpui::test]
fn a_files_checkbox_stages_and_unstages_it(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (_workbench, panel, _, cx) = open(cx, root);
    assert_eq!(
        groups(&panel, cx),
        [
            ("Staged Changes".to_owned(), vec!["M a.rs".to_owned()]),
            (
                "Changes".to_owned(),
                vec![
                    "M b.rs".to_owned(),
                    "M c.rs".to_owned(),
                    "D d.rs".to_owned(),
                    "U e.rs".to_owned()
                ]
            ),
        ]
    );
    // Checked is staged.
    let checked = panel.read_with(cx, |panel, _| {
        (0..5)
            .map(|ix| panel.checked(Some(ix), Group::Changes))
            .collect::<Vec<_>>()
    });
    assert_eq!(checked, [true, false, false, false, false]);

    // Checking b.rs stages it; the row's click doesn't open its diff.
    click_checkbox(&panel, Some(1), Group::Changes, Action::Stage, cx);
    assert_eq!(
        status(root),
        ["M  a.rs", "M  b.rs", " M c.rs", " D d.rs", "?? e.rs"]
    );
    let staged = groups(&panel, cx)[0].1.clone();
    assert_eq!(staged, ["M a.rs", "M b.rs"]);

    // Unchecking a staged row unstages it.
    click_checkbox(&panel, Some(0), Group::Staged, Action::Unstage, cx);
    let staged = groups(&panel, cx)[0].1.clone();
    assert_eq!(staged, ["M b.rs"]);
    assert!(status(root).contains(&" M a.rs".to_owned()));

    // Open and Discard stay as the row's buttons; staging is the checkbox.
    let buttons = panel.read_with(cx, |panel, _| {
        (
            panel.row_buttons(Some(0), Group::Staged),
            panel.row_buttons(Some(1), Group::Changes),
        )
    });
    assert_eq!(buttons.0, [Action::OpenFile]);
    assert_eq!(buttons.1, [Action::OpenFile, Action::Discard]);
}

#[gpui::test]
fn a_groups_checkbox_checks_all(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (_workbench, panel, _, cx) = open(cx, root);
    let headers = panel.read_with(cx, |panel, _| {
        (
            panel.checked(None, Group::Staged),
            panel.checked(None, Group::Changes),
        )
    });
    assert_eq!(headers, (true, false));

    // Checking Changes' header stages every change, the deletion and the
    // new file too.
    click_checkbox(&panel, None, Group::Changes, Action::Stage, cx);
    assert_eq!(
        status(root),
        ["M  a.rs", "M  b.rs", "M  c.rs", "D  d.rs", "A  e.rs"]
    );
    let shown = groups(&panel, cx);
    assert_eq!(shown[0].1.len(), 5);
    assert_eq!(shown[1], ("Changes".to_owned(), Vec::new()));

    // Unchecking Staged Changes' header unstages them all.
    click_checkbox(&panel, None, Group::Staged, Action::Unstage, cx);
    assert_eq!(
        status(root),
        [" M a.rs", " M b.rs", " M c.rs", " D d.rs", "?? e.rs"]
    );
    let titles: Vec<_> = (groups(&panel, cx).into_iter())
        .map(|(title, _)| title)
        .collect();
    assert_eq!(titles, ["Changes"]);
}

#[gpui::test]
fn letters_and_colours_come_from_vcs(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_workbench, panel, _, cx) = open(cx, dir.path());
    panel.read_with(cx, |_, cx| {
        let vcs = cx.theme().colors.vcs;
        for (kind, letter, color) in [
            (ChangeKind::Modified, "M", vcs.modified),
            (ChangeKind::Added, "A", vcs.added),
            (ChangeKind::Deleted, "D", vcs.deleted),
            (ChangeKind::Untracked, "U", vcs.untracked),
            (ChangeKind::Conflicted, "C", vcs.conflict),
        ] {
            assert_eq!(git_panel::letter(kind), letter);
            assert_eq!(git_panel::letter_color(kind, cx), color, "{kind:?}");
        }
    });
    // The hue added files were turned to is gone.
    let source = include_str!("../git_panel.rs");
    assert!(!source.contains(concat!("ADDED", "_HUE")));
    // Each row has its letter, so the colour is never the only cue.
    let rows = panel.read_with(cx, |panel, _| panel.shown_rows());
    let letters: Vec<_> = rows.iter().map(|row| row.letter).collect();
    assert_eq!(letters, ["M", "M", "M", "D", "U"]);
}

#[gpui::test]
fn the_placeholder_is_in_the_ui_font(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_workbench, panel, commit_box, cx) = open(cx, dir.path());
    panel.read_with(cx, |panel, _| panel.take_drawn_placeholder());
    draw(cx);
    let drawn = panel.read_with(cx, |panel, _| panel.take_drawn_placeholder());
    let (text, family) = drawn.expect("the empty box's placeholder is drawn");
    assert_eq!(text, "Message");
    let ui = cx.update(|_, cx| {
        (cx.try_global::<FontFamilies>().cloned().unwrap_or_default()).family(FamilySlot::Ui, cx)
    });
    assert_eq!(family, ui);
    let code = commit_box.read_with(cx, |editor, _| editor.theme().font_family.clone());
    assert_ne!(family, code);
    // It's over the box, not text in it: the editor paints nothing after
    // its empty row.
    let placeholder = cx.debug_bounds("commit-placeholder").expect("drawn");
    let field = cx.debug_bounds("commit-box").expect("drawn");
    assert!(
        field.contains(&placeholder.origin),
        "{placeholder:?} in {field:?}"
    );
    let decorations = commit_box.read_with(cx, |editor, _| editor.model().decorations(0..0));
    assert!(decorations.is_empty(), "{decorations:?}");

    // Typing hides it.
    set_text(&commit_box, "Fix", cx);
    panel.read_with(cx, |panel, _| panel.take_drawn_placeholder());
    draw(cx);
    let drawn = panel.read_with(cx, |panel, _| panel.take_drawn_placeholder());
    assert_eq!(drawn, None);
    assert!(cx.debug_bounds("commit-placeholder").is_none());
}

#[gpui::test]
fn the_counter_turns_error_past_72(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_workbench, panel, commit_box, cx) = open(cx, dir.path());
    let drawn = |panel: &Entity<GitPanel>, cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, _| panel.take_drawn_counter());
        draw(cx);
        panel.read_with(cx, |panel, _| panel.take_drawn_counter())
    };
    let (faint, error, ui) = cx.update(|_, cx| {
        let colors = cx.theme().colors;
        let ui = (cx.try_global::<FontFamilies>().cloned().unwrap_or_default())
            .family(FamilySlot::Ui, cx);
        (colors.text.faint, colors.status.error, ui)
    });
    assert_eq!(LINE_LENGTH, 72);

    // None on an empty box.
    assert_eq!(drawn(&panel, cx), None);

    // 72 is faint; 73 is in the error colour.
    let subject = "s".repeat(72);
    set_text(&commit_box, &subject, cx);
    set_caret(&commit_box, 72, cx);
    assert_eq!(drawn(&panel, cx), Some(("72/72".into(), faint, ui.clone())));
    set_text(&commit_box, &format!("{subject}s"), cx);
    set_caret(&commit_box, 73, cx);
    assert_eq!(drawn(&panel, cx), Some(("73/72".into(), error, ui.clone())));
    assert!(cx.debug_bounds("commit-counter").is_some());

    // One counter, for the caret's row: a short body row is faint.
    set_text(&commit_box, &format!("{subject}s\n\nBody"), cx);
    set_caret(&commit_box, subject.len() + 2, cx);
    assert_eq!(drawn(&panel, cx), Some(("0/72".into(), faint, ui.clone())));
    set_caret(&commit_box, subject.len() + 7, cx);
    assert_eq!(drawn(&panel, cx), Some(("4/72".into(), faint, ui)));
}

#[gpui::test]
fn the_button_reads_commit_n_files_with_its_key(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (_workbench, panel, commit_box, cx) = open(cx, root);
    set_text(&commit_box, "Fix it", cx);
    let key = commit_key(cx);
    let button = panel
        .read_with(cx, |panel, cx| panel.commit_button(cx))
        .unwrap();
    assert_eq!(button.label.resolve(), "Commit 1 file");
    assert_eq!(button.key.as_deref(), Some(key.as_str()));
    assert_eq!(button.text().resolve(), format!("Commit 1 file {key}"));
    assert!(button.enabled);

    git(root, &["add", "b.rs", "c.rs"]);
    panel.update(cx, |panel, cx| panel.refresh(cx));
    draw(cx);
    let button = panel
        .read_with(cx, |panel, cx| panel.commit_button(cx))
        .unwrap();
    assert_eq!(button.text().resolve(), format!("Commit 3 files {key}"));

    // With nothing staged it commits by asking to stage everything.
    git(root, &["reset", "-q"]);
    panel.update(cx, |panel, cx| panel.refresh(cx));
    draw(cx);
    let button = panel
        .read_with(cx, |panel, cx| panel.commit_button(cx))
        .unwrap();
    assert_eq!(button.label.resolve(), "Commit");
    // Disabled, the key isn't offered.
    set_text(&commit_box, "", cx);
    let button = panel
        .read_with(cx, |panel, cx| panel.commit_button(cx))
        .unwrap();
    assert!(!button.enabled);
    assert_eq!(button.text().resolve(), "Commit");
}

#[gpui::test]
fn and_push_commits_then_runs_git_push(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "-q", "--bare"]);
    let url = remote.path().to_str().unwrap();
    git(root, &["remote", "add", "origin", url]);
    let (workbench, panel, commit_box, cx) = open(cx, root);
    workbench.update(cx, |workbench, _| workbench.git_routed.clear());

    set_text(&commit_box, "Ship it", cx);
    let bounds = cx.debug_bounds("commit-and-push").expect("drawn");
    // In the panel, with the box above it.
    let field = cx.debug_bounds("commit-box").expect("drawn");
    assert!(
        bounds.right() <= field.right() + gpui::px(1.),
        "{bounds:?} past {field:?}"
    );
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
    // The commit is made, then pushed: the remote's main is HEAD.
    assert_eq!(git(root, &["log", "-1", "--format=%s"]), "Ship it");
    let head = git(root, &["rev-parse", "HEAD"]);
    assert_eq!(git(remote.path(), &["rev-parse", "main"]), head);
    let routed = workbench.read_with(cx, |workbench, _| workbench.git_routed.clone());
    let pushes: Vec<_> = (routed.iter())
        .filter(|routed| matches!(routed, Routed::Op(op) if op.command == GitCommand::Push))
        .collect();
    assert_eq!(pushes.len(), 1, "{routed:?}");
    let text = commit_box.read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(text, "");

    // The Commit button alone doesn't push.
    workbench.update(cx, |workbench, _| workbench.git_routed.clear());
    git(root, &["add", "b.rs"]);
    panel.update(cx, |panel, cx| panel.refresh(cx));
    set_text(&commit_box, "Local only", cx);
    let bounds = cx.debug_bounds("commit-button").expect("drawn");
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
    assert_eq!(git(root, &["log", "-1", "--format=%s"]), "Local only");
    assert_eq!(git(remote.path(), &["rev-parse", "main"]), head);
    let routed = workbench.read_with(cx, |workbench, _| workbench.git_routed.clone());
    assert!(
        !(routed.iter())
            .any(|routed| matches!(routed, Routed::Op(op) if op.command == GitCommand::Push)),
        "{routed:?}"
    );
}

/// A fake locale, `qps-ploc`, for this binary: the Changes panel's title.
static FAKE_LOCALE: kiln_l10n::Resources = kiln_l10n::Resources {
    crate_name: "fe-workbench-changes-fake-locale",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[kiln_l10n::ResourceFile {
        locale: "qps-ploc",
        path: "qps-ploc/changes.ftl",
        source: "workbench-changes-title = ⟦Çháñgés⟧\n",
    }],
    dynamic_prefixes: &[],
};

kiln_l10n::__private::inventory::submit! { kiln_l10n::Registered(&FAKE_LOCALE) }

#[gpui::test]
fn the_button_counts_what_a_commit_takes_from_the_whole_repository(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    // The project is a folder of the repository: one staged file in it,
    // and a.rs, staged, outside it.
    fs::create_dir(root.join("sub")).unwrap();
    fs::write(root.join("sub/in.rs"), "in\n").unwrap();
    git(root, &["add", "sub/in.rs"]);
    let (_workbench, panel, commit_box, cx) = open(cx, &root.join("sub"));
    let staged = groups(&panel, cx)[0].clone();
    assert_eq!(
        staged,
        ("Staged Changes".to_owned(), vec!["A in.rs".to_owned()])
    );

    set_text(&commit_box, "Both", cx);
    let button = panel
        .read_with(cx, |panel, cx| panel.commit_button(cx))
        .unwrap();
    assert_eq!(button.label.resolve(), "Commit 2 files");
    let bounds = cx.debug_bounds("commit-button").expect("drawn");
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
    let committed = git(root, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(committed, "a.rs\nsub/in.rs");
}

#[gpui::test]
fn a_long_names_letter_stays_in_its_row(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let long = format!("{}.rs", "a_very_long_file_name".repeat(6));
    fs::write(root.join(&long), "new\n").unwrap();
    let (_workbench, panel, _, cx) = open(cx, root);
    let ix = (panel.read_with(cx, |panel, _| panel.shown_rows()).iter())
        .position(|row| row.name == long)
        .expect("listed");
    let (row, selector) = panel.read_with(cx, |panel, _| {
        (
            panel.row_bounds(ix).expect("drawn"),
            panel.letter_selector(ix),
        )
    });
    let letter = cx
        .debug_bounds(selector.leak())
        .expect("the letter is drawn");
    assert!(letter.size.width > gpui::px(0.), "{letter:?}");
    assert!(
        letter.left() >= row.left() && letter.right() <= row.right(),
        "{letter:?} outside {row:?}"
    );
}

#[gpui::test]
fn the_titles_follow_the_language(cx: &mut TestAppContext) {
    let dir = fixture();
    let (workbench, _, _, cx) = open(cx, dir.path());
    let titles = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            let container = workbench.dock().read(cx).source_control().clone();
            let title = kiln_shell::DockItem::title(container.read(cx), cx);
            let view = (container.read(cx).stack().read(cx))
                .title(crate::dock::source_control::CHANGES)
                .expect("the Changes view");
            (title.to_string(), view.to_string())
        })
    };
    assert_eq!(titles(cx), ("Changes".to_owned(), "Changes".to_owned()));

    let fake: Vec<kiln_l10n::LanguageIdentifier> = vec!["qps-ploc".parse().unwrap()];
    cx.update(|_, cx| kiln_platform::locale::set_locale(cx, &fake));
    draw(cx);
    assert_eq!(titles(cx), ("⟦Çháñgés⟧".to_owned(), "⟦Çháñgés⟧".to_owned()));

    cx.update(|_, cx| kiln_platform::locale::set_locale(cx, &[]));
    draw(cx);
    assert_eq!(titles(cx), ("Changes".to_owned(), "Changes".to_owned()));
}
