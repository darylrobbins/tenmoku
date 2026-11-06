//! The Changes panel's Staged Changes and Changes groups, staging,
//! unstaging and discarding from its buttons, its vim keys and the git
//! commands, on repositories built with the `git` CLI in temporary
//! directories.

use std::{fs, path::Path, process::Command};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_git::{ChangeKind, Repo};
use fe_gpui::{
    DiffView, EditorSettings, EditorTheme, EditorView, commands::RunCommand, set_vim_enabled,
};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::{Action, GitPanel, Group, ShownGroup};
use crate::{Launch, Workbench};

/// Runs `git` in `dir`, with the developer's own config shut out.
fn git(dir: &Path, args: &[&str]) {
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
}

fn write(root: &Path, rela: &str, contents: &str) {
    fs::write(root.join(rela), contents).unwrap();
}

fn read(root: &Path, rela: &str) -> String {
    fs::read_to_string(root.join(rela)).unwrap()
}

/// A repository whose `a.rs` is staged, `b.rs` changed, `c.rs` staged and
/// changed again, and `d.rs` untracked: `a.rs` and `c.rs` are `index\n` in
/// the index, every committed file `head\n` in HEAD.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    for file in ["a.rs", "b.rs", "c.rs"] {
        write(root, file, "head\n");
    }
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    write(root, "a.rs", "index\n");
    write(root, "c.rs", "index\n");
    git(root, &["add", "a.rs", "c.rs"]);
    write(root, "b.rs", "working\n");
    write(root, "c.rs", "working\n");
    write(root, "d.rs", "new\n");
    dir
}

/// A workbench for `root`, in a headless window, focused as the app
/// focuses it, showing the Changes panel.
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
    draw(cx);
    dispatch("git.show_changes", cx);
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).changes().clone()
    });
    (workbench, panel, cx)
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

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

/// Types `:{line}` and Enter.
fn ex(line: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(":");
    cx.simulate_input(line);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

fn focus(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    draw(cx);
}

/// The groups shown: each one's title, count, and rows' letters and paths.
fn shown_groups(panel: &Entity<GitPanel>, cx: &mut VisualTestContext) -> Vec<ShownGroup> {
    panel.read_with(cx, |panel, _| panel.groups())
}

/// What the Staged Changes and Changes groups list: each row's letter and
/// path, in a group of its own.
fn rows(staged: &[(&'static str, &str)], changes: &[(&'static str, &str)]) -> Vec<ShownGroup> {
    let rows = |rows: &[(&'static str, &str)]| {
        (rows.iter())
            .map(|(letter, path)| (*letter, (*path).to_owned()))
            .collect::<Vec<_>>()
    };
    let mut shown = Vec::new();
    if !staged.is_empty() {
        shown.push(("Staged Changes", staged.len(), rows(staged)));
    }
    shown.push(("Changes", changes.len(), rows(changes)));
    shown
}

/// Each changed file's name and its staged and unstaged halves, as git
/// reads them now.
fn status(root: &Path) -> Vec<(String, Option<ChangeKind>, Option<ChangeKind>)> {
    let repo = Repo::discover(root).unwrap().unwrap();
    (repo.status_split().unwrap().into_iter())
        .map(|status| {
            let name = status.path.file_name().unwrap().to_string_lossy();
            (name.into_owned(), status.staged, status.unstaged)
        })
        .collect()
}

fn index_text(root: &Path, rela: &str) -> Option<String> {
    let repo = Repo::discover(root).unwrap().unwrap();
    repo.index_text(&root.join(rela)).unwrap()
}

/// Clicks the button for `action` on file `ix`'s row, or with `ix` `None`
/// on `group`'s header, with the pointer over the row first, as its buttons
/// show then.
fn click_button(
    panel: &Entity<GitPanel>,
    ix: Option<usize>,
    group: Group,
    action: Action,
    cx: &mut VisualTestContext,
) {
    let row = panel.read_with(cx, |panel, _| match ix {
        Some(ix) => panel.row_bounds(ix),
        None => panel.header_bounds(group),
    });
    cx.simulate_mouse_move(row.expect("drawn").center(), None, Modifiers::none());
    draw(cx);
    let selector = panel.read_with(cx, |panel, _| panel.button_selector(ix, group, action));
    let bounds = cx.debug_bounds(selector.leak()).expect("shown");
    cx.simulate_click(bounds.center(), Modifiers::none());
    draw(cx);
}

/// The active pane's active tab's title, and its diff's sides' texts.
fn shown_diff(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (String, [String; 2]) {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let item = pane.active_item().expect("a tab");
        let title = item.tab_title(cx).to_string();
        let diff = item.downcast_ref::<DiffView>().expect("a diff").read(cx);
        let text = |side: &Entity<EditorView>| side.read(cx).model().text().to_string();
        (title, [text(diff.left()), text(diff.right())])
    })
}

fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let detail = entry.toast.detail.clone().unwrap_or_default();
                (entry.toast.message.clone(), detail)
            })
            .collect()
    })
}

/// Opens `rela` under the root in the active pane and focuses it.
fn open_file(
    workbench: &Entity<Workbench>,
    root: &Path,
    rela: &str,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = root.join(rela);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("an editor");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    editor
}

#[gpui::test]
fn groups(cx: &mut TestAppContext) {
    let dir = fixture();
    let (_, panel, cx) = open(cx, dir.path());
    assert_eq!(
        shown_groups(&panel, cx),
        rows(
            &[("M", "a.rs"), ("M", "c.rs")],
            &[("M", "b.rs"), ("M", "c.rs"), ("U", "d.rs")]
        )
    );
    // The project tree has each file once, its halves combined.
    let tree: Vec<(String, ChangeKind)> = panel.read_with(cx, |panel, _| {
        (panel.statuses())
            .map(|(path, kind)| (path.display().to_string(), kind))
            .collect()
    });
    let modified = |name: &str| (name.to_owned(), ChangeKind::Modified);
    let untracked = ("d.rs".to_owned(), ChangeKind::Untracked);
    assert_eq!(
        tree,
        [
            modified("a.rs"),
            modified("b.rs"),
            modified("c.rs"),
            untracked
        ]
    );
    // Both headers are drawn, Staged Changes first.
    let (staged, changes) = panel.read_with(cx, |panel, _| {
        let header = |group| panel.header_bounds(group).expect("drawn");
        (header(Group::Staged), header(Group::Changes))
    });
    assert!(staged.bottom() <= changes.top(), "{staged:?} {changes:?}");

    // Nothing staged: no Staged Changes group.
    git(dir.path(), &["reset", "-q"]);
    dispatch("git.refresh", cx);
    assert_eq!(
        shown_groups(&panel, cx),
        rows(
            &[],
            &[("M", "a.rs"), ("M", "b.rs"), ("M", "c.rs"), ("U", "d.rs")]
        )
    );
    assert!(panel.read_with(cx, |panel, _| panel.header_bounds(Group::Staged).is_none()));
}

#[gpui::test]
fn stage_unstage(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, panel, cx) = open(cx, root);

    // Enter on a Staged row: HEAD against the index.
    keys("down", cx);
    keys("enter", cx);
    let staged = shown_diff(&workbench, cx);
    assert_eq!(
        staged,
        (
            "a.rs (HEAD ↔ Index)".into(),
            ["head\n".into(), "index\n".into()]
        )
    );
    // On a Changes row: the index against the file.
    focus(&panel, cx);
    keys("down down down", cx);
    keys("enter", cx);
    let changed = shown_diff(&workbench, cx);
    assert_eq!(
        changed,
        (
            "c.rs (Index ↔ Working Tree)".into(),
            ["index\n".into(), "working\n".into()]
        )
    );

    // Rows: a.rs, c.rs staged; b.rs, c.rs, d.rs changed.
    click_button(&panel, Some(2), Group::Changes, Action::Stage, cx);
    let modified = Some(ChangeKind::Modified);
    let b = ("b.rs".to_owned(), modified, None);
    assert!(status(root).contains(&b), "{:?}", status(root));
    assert_eq!(
        shown_groups(&panel, cx),
        rows(
            &[("M", "a.rs"), ("M", "b.rs"), ("M", "c.rs")],
            &[("M", "c.rs"), ("U", "d.rs")]
        )
    );

    click_button(&panel, Some(0), Group::Staged, Action::Unstage, cx);
    let a = ("a.rs".to_owned(), None, modified);
    assert!(status(root).contains(&a), "{:?}", status(root));
    assert_eq!(
        shown_groups(&panel, cx),
        rows(
            &[("M", "b.rs"), ("M", "c.rs")],
            &[("M", "a.rs"), ("M", "c.rs"), ("U", "d.rs")]
        )
    );

    click_button(&panel, None, Group::Changes, Action::Stage, cx);
    let unstaged: Vec<_> = (status(root).into_iter())
        .filter(|(_, _, unstaged)| unstaged.is_some())
        .collect();
    assert_eq!(unstaged, []);
    assert_eq!(
        shown_groups(&panel, cx),
        rows(
            &[("M", "a.rs"), ("M", "b.rs"), ("M", "c.rs"), ("A", "d.rs")],
            &[]
        )
    );

    click_button(&panel, None, Group::Staged, Action::Unstage, cx);
    let staged: Vec<_> = (status(root).into_iter())
        .filter(|(_, staged, _)| staged.is_some())
        .collect();
    assert_eq!(staged, []);
    assert_eq!(
        shown_groups(&panel, cx),
        rows(
            &[],
            &[("M", "a.rs"), ("M", "b.rs"), ("M", "c.rs"), ("U", "d.rs")]
        )
    );
    assert!(toasts(&workbench, cx).is_empty());
}

#[gpui::test]
fn discard_confirms(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (_, panel, cx) = open(cx, root);
    // c.rs is index\n in the index, working\n on disk: the Changes row 3.
    click_button(&panel, Some(3), Group::Changes, Action::Discard, cx);
    let (question, detail) = cx.pending_prompt().expect("a question");
    assert_eq!(
        question,
        "Are you sure you want to discard changes in c.rs?"
    );
    assert_eq!(detail, "");
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert_eq!(read(root, "c.rs"), "working\n");
    assert_eq!(index_text(root, "c.rs").as_deref(), Some("index\n"));

    click_button(&panel, Some(3), Group::Changes, Action::Discard, cx);
    cx.simulate_prompt_answer("Discard File");
    draw(cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!(read(root, "c.rs"), "index\n");
    assert_eq!(
        shown_groups(&panel, cx),
        rows(
            &[("M", "a.rs"), ("M", "c.rs")],
            &[("M", "b.rs"), ("U", "d.rs")]
        )
    );

    // Untracked: deleted, if you're sure.
    click_button(&panel, Some(3), Group::Changes, Action::Discard, cx);
    let (question, detail) = cx.pending_prompt().expect("a question");
    assert_eq!(question, "Are you sure you want to DELETE d.rs?");
    assert_eq!(
        detail,
        "This is IRREVERSIBLE! This file will be FOREVER LOST if you proceed."
    );
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert!(root.join("d.rs").exists());
    click_button(&panel, Some(3), Group::Changes, Action::Discard, cx);
    cx.simulate_prompt_answer("Delete File");
    draw(cx);
    assert!(!root.join("d.rs").exists());
    assert_eq!(
        shown_groups(&panel, cx),
        rows(&[("M", "a.rs"), ("M", "c.rs")], &[("M", "b.rs")])
    );

    // Several: counted.
    write(root, "a.rs", "again\n");
    dispatch("git.refresh", cx);
    click_button(&panel, None, Group::Changes, Action::Discard, cx);
    let (question, _) = cx.pending_prompt().expect("a question");
    assert_eq!(
        question,
        "Are you sure you want to discard changes in 2 files?"
    );
    cx.simulate_prompt_answer("Discard 2 Files");
    draw(cx);
    assert_eq!(
        (read(root, "a.rs"), read(root, "b.rs")),
        ("index\n".into(), "head\n".into())
    );
}

#[gpui::test]
fn vim_and_ex(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, panel, cx) = open(cx, root);
    let selected = |cx: &mut VisualTestContext| panel.read_with(cx, |panel, _| panel.selected());

    // With vim off, fugitive's letters do nothing.
    focus(&panel, cx);
    keys("j j", cx);
    assert_eq!(selected(cx), None);
    keys("down down", cx);
    keys("s u - X k", cx);
    assert_eq!(selected(cx), Some(1));
    assert!(!cx.has_pending_prompt());
    let before = status(root);
    assert_eq!(before.len(), 4);

    cx.update(|_, cx| set_vim_enabled(cx, true));
    focus(&panel, cx);
    keys("j", cx);
    assert_eq!(selected(cx), Some(2));
    keys("k k", cx);
    assert_eq!(selected(cx), Some(0));

    // Rows: a.rs, c.rs staged; b.rs, c.rs, d.rs changed. `s` on b.rs.
    keys("j j", cx);
    keys("s", cx);
    let modified = Some(ChangeKind::Modified);
    assert!(status(root).contains(&("b.rs".to_owned(), modified, None)));
    // `u` on a.rs, staged; the row now where b.rs was is selected.
    assert_eq!(selected(cx), Some(3));
    focus(&panel, cx);
    keys("k k k k", cx);
    assert_eq!(selected(cx), Some(0));
    keys("u", cx);
    assert!(status(root).contains(&("a.rs".to_owned(), None, modified)));
    // Rows: b.rs, c.rs staged; a.rs, c.rs, d.rs changed. `-` on d.rs
    // stages it, and on it again, now staged, unstages it.
    focus(&panel, cx);
    keys("j j j j", cx);
    assert_eq!(selected(cx), Some(4));
    keys("-", cx);
    let added = ("d.rs".to_owned(), Some(ChangeKind::Added), None);
    assert!(status(root).contains(&added), "{:?}", status(root));
    focus(&panel, cx);
    keys("k k", cx);
    assert_eq!(selected(cx), Some(2));
    keys("-", cx);
    let untracked = ("d.rs".to_owned(), None, Some(ChangeKind::Untracked));
    assert!(status(root).contains(&untracked), "{:?}", status(root));
    // `X` asks before it discards: the selected row's now c.rs, staged.
    focus(&panel, cx);
    assert_eq!(selected(cx), Some(1));
    keys("shift-x", cx);
    let (question, _) = cx.pending_prompt().expect("a question");
    assert_eq!(
        question,
        "Are you sure you want to discard changes in c.rs?"
    );
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert_eq!(read(root, "c.rs"), "working\n");

    // `:Gwrite` saves, then stages.
    let editor = open_file(&workbench, root, "b.rs", cx);
    keys("g g i x escape", cx);
    ex("Gwrite", cx);
    assert_eq!(read(root, "b.rs"), "xworking\n");
    assert_eq!(index_text(root, "b.rs").as_deref(), Some("xworking\n"));
    assert!(!editor.read_with(cx, |editor, _| editor.model().is_dirty()));

    // `:Gread` takes the index's text, undoably and unsaved.
    let editor = open_file(&workbench, root, "c.rs", cx);
    ex("Gread", cx);
    let text = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            (editor.model().text().to_string(), editor.model().is_dirty())
        })
    };
    assert_eq!(text(cx), ("index\n".into(), true));
    assert_eq!(read(root, "c.rs"), "working\n");
    keys("u", cx);
    assert_eq!(text(cx), ("working\n".into(), false));
}

#[gpui::test]
fn a_git_failure_shows_its_error(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, panel, cx) = open(cx, root);
    write(root, ".git/index.lock", "");
    click_button(&panel, Some(2), Group::Changes, Action::Stage, cx);
    let shown = toasts(&workbench, cx);
    let [(message, detail)] = shown.as_slice() else {
        panic!("one toast: {shown:?}");
    };
    assert!(
        message.starts_with("Git: fatal: Unable to create") && message.contains("index.lock"),
        "{message}"
    );
    assert!(detail.contains("Another git process"), "{detail}");
}

/// Makes `text` the index's content for `rela`, the file on disk as it is.
fn set_index(root: &Path, rela: &str, text: &str) {
    let repo = Repo::discover(root).unwrap().unwrap();
    repo.set_index_text(&root.join(rela), text).unwrap();
}

/// What the index holds for `rela`, as `git cat-file` prints it: a
/// symlink's target.
#[cfg(unix)]
fn index_blob(root: &Path, rela: &str) -> String {
    let output = Command::new("git")
        .args(["cat-file", "-p", &format!(":{rela}")])
        .current_dir(root)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Runs workbench command `id` with focus where it is.
fn run_command(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn focus_editor(editor: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
}

/// The active tab's diff view.
fn active_diff(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<DiffView> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (pane.active_item())
            .and_then(|item| item.downcast_ref::<DiffView>().cloned())
            .expect("a diff")
    })
}

fn sides(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> [Entity<EditorView>; 2] {
    diff.read_with(cx, |diff, _| [diff.left().clone(), diff.right().clone()])
}

fn vim_message(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        editor.update(cx, |editor, cx| {
            let message = editor.vim_state()?.message().map(str::to_owned);
            editor.set_vim_message("", cx);
            message.filter(|message| !message.is_empty())
        })
    })
}

fn buffer_text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string())
}

#[gpui::test]
#[cfg(unix)]
fn symlinks_are_staged_and_discarded_as_links(cx: &mut TestAppContext) {
    use std::os::unix::fs::symlink;
    let dir = fixture();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("x.rs");
    fs::write(&target, "outside\n").unwrap();
    symlink(&target, root.join("link.rs")).unwrap();
    symlink(root.join("gone.rs"), root.join("dangling.rs")).unwrap();
    symlink("a.rs", root.join("tracked.rs")).unwrap();
    git(root, &["add", "tracked.rs"]);
    git(root, &["commit", "-q", "-m", "link"]);
    fs::remove_file(root.join("tracked.rs")).unwrap();
    symlink("b.rs", root.join("tracked.rs")).unwrap();
    let (_, panel, cx) = open(cx, root);
    let row = |name: &str, cx: &mut VisualTestContext| {
        let path = root.join(name);
        let changes = panel.read_with(cx, |panel, _| panel.paths());
        let staged = (shown_groups(&panel, cx).first()).map_or(0, |(title, count, _)| {
            if *title == "Staged Changes" {
                *count
            } else {
                0
            }
        });
        (changes
            .iter()
            .skip(staged)
            .position(|listed| *listed == path))
        .map(|ix| ix + staged)
        .expect("listed")
    };

    // A tracked link staged is the link, not the file it points to.
    let ix = row("tracked.rs", cx);
    click_button(&panel, Some(ix), Group::Changes, Action::Stage, cx);
    assert_eq!(index_blob(root, "tracked.rs"), "b.rs");
    assert_eq!(index_text(root, "b.rs").as_deref(), Some("head\n"));

    // An untracked link discarded is the link: what it points to stays.
    let ix = row("link.rs", cx);
    click_button(&panel, Some(ix), Group::Changes, Action::Discard, cx);
    let (question, _) = cx.pending_prompt().expect("a question");
    assert_eq!(question, "Are you sure you want to DELETE link.rs?");
    cx.simulate_prompt_answer("Delete File");
    draw(cx);
    assert!(root.join("link.rs").symlink_metadata().is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "outside\n");

    // A dangling one too.
    let ix = row("dangling.rs", cx);
    click_button(&panel, Some(ix), Group::Changes, Action::Discard, cx);
    cx.simulate_prompt_answer("Delete File");
    draw(cx);
    assert!(root.join("dangling.rs").symlink_metadata().is_err());
}

#[gpui::test]
fn discard_reads_what_the_file_is_before_asking(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    fs::create_dir(root.join("sub")).unwrap();
    write(root, "sub/s.rs", "s\n");
    git(root, &["add", "sub/s.rs"]);
    git(root, &["commit", "-q", "-m", "sub"]);
    let sub = root.join("sub");
    let (workbench, _, cx) = open(cx, &sub);
    set_vim_enabled_in(cx);

    // Untracked, outside the root, the panel never listed it, opened
    // through `..`: still deleted only once you're sure you want to.
    write(root, "top.rs", "top\n");
    let editor = open_file(&workbench, &sub, "../top.rs", cx);
    run_command("git.discard_file", cx);
    let (question, detail) = cx.pending_prompt().expect("a question");
    assert_eq!(question, "Are you sure you want to DELETE top.rs?");
    assert_eq!(
        detail,
        "This is IRREVERSIBLE! This file will be FOREVER LOST if you proceed."
    );
    cx.simulate_prompt_answer("Delete File");
    draw(cx);
    assert!(!root.join("top.rs").exists());
    drop(editor);

    // Tracked when asked, untracked by the time you're sure: left alone.
    let editor = open_file(&workbench, root, "c.rs", cx);
    run_command("git.discard_file", cx);
    let (question, _) = cx.pending_prompt().expect("a question");
    assert_eq!(
        question,
        "Are you sure you want to discard changes in c.rs?"
    );
    git(root, &["rm", "-q", "--cached", "c.rs"]);
    cx.simulate_prompt_answer("Discard File");
    draw(cx);
    assert_eq!(read(root, "c.rs"), "working\n");
    assert_eq!(
        toasts(&workbench, cx),
        [(
            "Changed since you were asked, so not discarded: c.rs".to_owned(),
            String::new()
        )]
    );

    // Nothing to discard: nothing asked.
    let editor_a = open_file(&workbench, root, "e.rs", cx);
    drop((editor, editor_a));
    assert!(!cx.has_pending_prompt());
}

impl Workbench {
    /// Holds the git work started from now.
    fn hold_git_work_in(cx: &mut VisualTestContext) {
        cx.update(|_, cx| Workbench::hold_git_work(cx));
    }
}

/// Turns vim on.
fn set_vim_enabled_in(cx: &mut VisualTestContext) {
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
}

#[gpui::test]
fn a_focused_panel_with_nothing_selected_acts_on_nothing(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, panel, cx) = open(cx, root);
    open_file(&workbench, root, "b.rs", cx);
    focus(&panel, cx);
    assert_eq!(panel.read_with(cx, |panel, _| panel.selected()), None);
    let before = status(root);
    for command in ["git.stage_file", "git.unstage_file", "git.discard_file"] {
        run_command(command, cx);
        assert!(!cx.has_pending_prompt(), "{command}");
        assert_eq!(status(root), before, "{command}");
    }
}

#[gpui::test]
fn a_staged_diff_stays_clean_as_the_index_changes(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, panel, cx) = open(cx, root);
    set_vim_enabled_in(cx);
    // Another tab, so the window stays open.
    open_file(&workbench, root, "b.rs", cx);
    focus(&panel, cx);
    keys("down", cx);
    keys("enter", cx);
    let diff = active_diff(&workbench, cx);
    set_index(root, "a.rs", "newer\n");
    dispatch("git.refresh", cx);
    let [left, right] = sides(&diff, cx);
    assert_eq!(
        (buffer_text(&left, cx), buffer_text(&right, cx)),
        ("head\n".into(), "newer\n".into())
    );
    assert!(!diff.read_with(cx, kiln_shell::PaneItem::is_dirty));
    focus_editor(&right, cx);
    ex("q", cx);
    assert_eq!(vim_message(&right, cx), None);
    let tabs = workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len());
    assert_eq!(tabs, 1);
}

#[gpui::test]
fn file_commands_in_a_staged_diff_act_on_its_file(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, panel, cx) = open(cx, root);
    set_vim_enabled_in(cx);
    focus(&panel, cx);
    keys("down", cx);
    keys("enter", cx);
    let diff = active_diff(&workbench, cx);
    let [left, right] = sides(&diff, cx);

    // `:Gread` in a read-only side: E21, as `:w` there.
    focus_editor(&left, cx);
    ex("Gread", cx);
    assert_eq!(
        vim_message(&left, cx).as_deref(),
        Some("E21: Cannot make changes, 'modifiable' is off")
    );

    focus_editor(&right, cx);
    ex("Gitsigns reset_buffer_index", cx);
    assert_eq!(vim_message(&right, cx), None);
    let modified = Some(ChangeKind::Modified);
    assert!(status(root).contains(&("a.rs".to_owned(), None, modified)));
    ex("Gwrite", cx);
    assert!(status(root).contains(&("a.rs".to_owned(), modified, None)));

    // Its file against the index, and against another file.
    focus_editor(&left, cx);
    ex("Gdiffsplit", cx);
    let title = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            pane.active_item().unwrap().tab_title(cx).to_string()
        })
    };
    assert_eq!(title(cx), "a.rs (Index ↔ Working Tree)");
    let staged = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (pane.tabs().iter()).position(|tab| tab.item().downcast_ref::<DiffView>() == Some(&diff))
    });
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.activate(staged.unwrap(), window, cx));
    });
    focus_editor(&right, cx);
    ex("diffsplit b.rs", cx);
    assert_eq!(title(cx), "a.rs (b.rs ↔ a.rs)");
    let [_, compared] = sides(&active_diff(&workbench, cx), cx);
    assert_eq!(buffer_text(&compared, cx), "index\n");
}

#[gpui::test]
fn gread_in_a_heads_side_answers_e21(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, _, cx) = open(cx, root);
    set_vim_enabled_in(cx);
    let tab = open_file(&workbench, root, "c.rs", cx);
    ex("Gdiffsplit HEAD", cx);
    let [left, _] = sides(&active_diff(&workbench, cx), cx);
    focus_editor(&left, cx);
    ex("Gread", cx);
    assert_eq!(
        vim_message(&left, cx).as_deref(),
        Some("E21: Cannot make changes, 'modifiable' is off")
    );
    assert_eq!(buffer_text(&tab, cx), "working\n");
}

#[gpui::test]
fn only_the_last_gread_lands_and_on_its_file(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, _, cx) = open(cx, root);
    set_vim_enabled_in(cx);
    let editor = open_file(&workbench, root, "c.rs", cx);
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    ex("Gread", cx);
    set_index(root, "c.rs", "newer\n");
    ex("Gread", cx);
    let held = |cx: &mut VisualTestContext| cx.update(|_, cx| Workbench::git_work_held(cx));
    assert_eq!(held(cx), 2);
    // The later read lands first; the earlier one, after it, is dropped.
    cx.update(|_, cx| Workbench::release_git_work(1, cx));
    draw(cx);
    assert_eq!(buffer_text(&editor, cx), "newer\n");
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(buffer_text(&editor, cx), "newer\n");

    // One landing after the editor moved to another file is dropped.
    keys("u", cx);
    assert_eq!(buffer_text(&editor, cx), "working\n");
    ex("Gread", cx);
    let moved = root.join("moved.rs");
    editor.update(cx, |editor, _| editor.model_mut().set_path(Some(moved)));
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(buffer_text(&editor, cx), "working\n");
}

#[gpui::test]
fn gread_is_an_undo_step_of_its_own(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, _, cx) = open(cx, root);
    set_vim_enabled_in(cx);
    let editor = open_file(&workbench, root, "c.rs", cx);
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    ex("Gread", cx);
    // Typing while the read is held: it lands mid-insert.
    keys("g g i x", cx);
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(buffer_text(&editor, cx), "index\n");
    keys("escape", cx);
    keys("u", cx);
    assert_eq!(buffer_text(&editor, cx), "xworking\n");
    keys("u", cx);
    assert_eq!(buffer_text(&editor, cx), "working\n");
}

#[gpui::test]
fn git_work_runs_in_the_order_asked(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, _, cx) = open(cx, root);
    let held = |cx: &mut VisualTestContext| cx.update(|_, cx| Workbench::git_work_held(cx));
    cx.update(|_, cx| Workbench::hold_git_work(cx));
    let b = root.join("b.rs");
    for stage in [true, false] {
        let path = b.clone();
        workbench.update(cx, |workbench, cx| {
            workbench.stage_files(vec![path], stage, cx)
        });
    }
    draw(cx);
    // The unstaging waits for the staging to finish.
    assert_eq!(held(cx), 1);
    let modified = Some(ChangeKind::Modified);
    assert!(status(root).contains(&("b.rs".to_owned(), None, modified)));
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert!(status(root).contains(&("b.rs".to_owned(), modified, None)));
    assert_eq!(held(cx), 1);
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert!(status(root).contains(&("b.rs".to_owned(), None, modified)));
    assert_eq!(held(cx), 0);
    assert!(toasts(&workbench, cx).is_empty());
}

#[gpui::test]
fn only_the_last_gread_of_a_shared_buffer_lands(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, _, cx) = open(cx, root);
    set_vim_enabled_in(cx);
    let tab = open_file(&workbench, root, "c.rs", cx);
    Workbench::hold_git_work_in(cx);
    ex("Gread", cx);
    set_index(root, "c.rs", "newer\n");
    // The diff's working side shares the tab's buffer.
    ex("Gdiffsplit", cx);
    let [_, right] = sides(&active_diff(&workbench, cx), cx);
    focus_editor(&right, cx);
    ex("Gread", cx);
    assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 2);
    cx.update(|_, cx| Workbench::release_git_work(1, cx));
    draw(cx);
    assert_eq!(buffer_text(&tab, cx), "newer\n");
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(buffer_text(&tab, cx), "newer\n");
}

#[gpui::test]
fn staging_from_a_staged_diff_saves_the_files_open_buffer(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, panel, cx) = open(cx, root);
    set_vim_enabled_in(cx);
    let tab = open_file(&workbench, root, "a.rs", cx);
    keys("g g i x escape", cx);
    focus(&panel, cx);
    keys("down", cx);
    keys("enter", cx);
    let [left, right] = sides(&active_diff(&workbench, cx), cx);

    focus_editor(&left, cx);
    ex("Gwrite", cx);
    assert_eq!(read(root, "a.rs"), "xindex\n");
    assert_eq!(index_text(root, "a.rs").as_deref(), Some("xindex\n"));
    assert!(!tab.read_with(cx, |tab, _| tab.model().is_dirty()));

    tab.update(cx, |tab, cx| {
        tab.model_mut()
            .buffer_mut()
            .edit([(0..0, "y")], fe_buffer::Origin::User);
        cx.notify();
    });
    focus_editor(&right, cx);
    ex("Gitsigns stage_buffer", cx);
    assert_eq!(read(root, "a.rs"), "yxindex\n");
    assert_eq!(index_text(root, "a.rs").as_deref(), Some("yxindex\n"));
}

#[gpui::test]
fn a_stage_waiting_for_its_save_keeps_its_place(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, _, cx) = open(cx, root);
    set_vim_enabled_in(cx);
    open_file(&workbench, root, "b.rs", cx);
    keys("g g i x escape", cx);
    // Stage, saving first, then unstage, before the save is done.
    let run = |id: &str| {
        RunCommand(
            Invocation::of(id)
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench"))),
        )
    };
    let (stage, unstage) = (run("git.stage_file"), run("git.unstage_file"));
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(stage), cx);
        window.dispatch_action(Box::new(unstage), cx);
    });
    draw(cx);
    assert_eq!(read(root, "b.rs"), "xworking\n");
    let modified = Some(ChangeKind::Modified);
    assert!(
        status(root).contains(&("b.rs".to_owned(), None, modified)),
        "{:?}",
        status(root)
    );
}

#[gpui::test]
fn compare_reads_the_file_if_its_tab_closes_meanwhile(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, _, cx) = open(cx, root);
    set_vim_enabled_in(cx);
    open_file(&workbench, root, "b.rs", cx);
    open_file(&workbench, root, "a.rs", cx);
    Workbench::hold_git_work_in(cx);
    ex("diffsplit b.rs", cx);
    assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1);
    ex("q", cx);
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    let diff = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        (pane.tabs().iter()).find_map(|tab| tab.item().downcast_ref::<DiffView>().cloned())
    });
    let [left, right] = sides(&diff.expect("the comparison"), cx);
    assert_eq!(
        (buffer_text(&left, cx), buffer_text(&right, cx)),
        ("working\n".into(), "index\n".into())
    );
}

#[gpui::test]
#[cfg(unix)]
fn staging_from_a_staged_diff_saves_the_buffer_opened_through_an_alias(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    // `a.rs` opened through a symlinked folder: another spelling of it.
    let elsewhere = tempfile::tempdir().unwrap();
    let alias = elsewhere.path().join("alias");
    std::os::unix::fs::symlink(root, &alias).unwrap();
    let (workbench, panel, cx) = open(cx, root);
    set_vim_enabled_in(cx);
    let tab = open_file(&workbench, &alias, "a.rs", cx);
    let tab_path = tab.read_with(cx, |tab, _| tab.model().path().map(Path::to_owned));
    assert_eq!(tab_path, Some(alias.join("a.rs")));
    focus(&panel, cx);
    keys("down", cx);
    keys("enter", cx);
    let [left, right] = sides(&active_diff(&workbench, cx), cx);

    // From each side, the unsaved text is saved, then staged.
    for (side, command, typed, expected) in [
        (&left, "Gwrite", "x", "xindex\n"),
        (&right, "Gitsigns stage_buffer", "y", "yxindex\n"),
    ] {
        tab.update(cx, |tab, cx| {
            tab.model_mut()
                .buffer_mut()
                .edit([(0..0, typed)], fe_buffer::Origin::User);
            cx.notify();
        });
        assert!(tab.read_with(cx, |tab, _| tab.model().is_dirty()));
        focus_editor(side, cx);
        ex(command, cx);
        assert_eq!(read(root, "a.rs"), expected, ":{command}");
        assert_eq!(
            index_text(root, "a.rs").as_deref(),
            Some(expected),
            ":{command}"
        );
        assert!(
            !tab.read_with(cx, |tab, _| tab.model().is_dirty()),
            ":{command}"
        );
    }
}

/// Runs `command`, a save and stage, in `c.rs`'s tab with a discard of it
/// confirmed and waiting its turn, and answers the save's question about
/// the file changed on disk with `answer`: the tab.
fn save_and_stage_after_a_discard(
    command: &str,
    answer: &str,
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<EditorView>) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, _, cx) = open(cx, root);
    set_vim_enabled_in(cx);
    let tab = open_file(&workbench, root, "c.rs", cx);
    Workbench::hold_git_work_in(cx);
    run_command("git.discard_file", cx);
    cx.simulate_prompt_answer("Discard File");
    draw(cx);
    assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1);
    keys("g g i x escape", cx);
    ex(command, cx);
    // Nothing saved before the discard has run.
    assert_eq!(read(root, "c.rs"), "working\n");
    cx.update(|_, cx| Workbench::release_git_work(0, cx));
    draw(cx);
    assert_eq!(read(root, "c.rs"), "index\n", "discarded first");
    let (question, _) = cx.pending_prompt().expect("the save's question");
    assert!(question.contains("changed on disk"), "{question}");
    cx.simulate_prompt_answer(answer);
    draw(cx);
    while cx.update(|_, cx| Workbench::git_work_held(cx)) > 0 {
        cx.update(|_, cx| Workbench::release_git_work(0, cx));
        draw(cx);
    }
    (dir, tab)
}

#[gpui::test]
fn gwrite_after_a_pending_discard_saves_in_its_turn(cx: &mut TestAppContext) {
    let (dir, tab) = save_and_stage_after_a_discard("Gwrite", "Overwrite", cx);
    let root = dir.path();
    // The buffer's text, saved after the discard, is what's staged.
    assert_eq!(read(root, "c.rs"), "xworking\n");
    assert_eq!(index_text(root, "c.rs").as_deref(), Some("xworking\n"));
    drop(tab);
}

#[gpui::test]
fn stage_buffer_after_a_pending_discard_stages_nothing_unsaved(cx: &mut TestAppContext) {
    let (dir, tab) = save_and_stage_after_a_discard("Gitsigns stage_buffer", "Cancel", cx);
    let root = dir.path();
    // Not saved, so nothing staged: the index keeps its text.
    assert_eq!(read(root, "c.rs"), "index\n");
    assert_eq!(index_text(root, "c.rs").as_deref(), Some("index\n"));
    drop(tab);
}

#[gpui::test]
#[cfg(unix)]
fn a_staged_symlinks_diff_compares_its_targets(cx: &mut TestAppContext) {
    use std::os::unix::fs::symlink;
    let dir = fixture();
    let root = dir.path();
    let outside = tempfile::tempdir().unwrap();
    let (one, two) = (outside.path().join("one"), outside.path().join("two"));
    symlink("a.rs", root.join("tracked.rs")).unwrap();
    symlink(&one, root.join("ext.rs")).unwrap();
    git(root, &["add", "tracked.rs", "ext.rs"]);
    git(root, &["commit", "-q", "-m", "links"]);
    for (link, target) in [("tracked.rs", Path::new("b.rs")), ("ext.rs", two.as_path())] {
        fs::remove_file(root.join(link)).unwrap();
        symlink(target, root.join(link)).unwrap();
    }
    git(root, &["add", "tracked.rs", "ext.rs"]);
    let (workbench, panel, cx) = open(cx, root);
    let open_row = |name: &str, cx: &mut VisualTestContext| {
        let path = root.join(name);
        let ix = panel.read_with(cx, |panel, _| panel.paths().iter().position(|p| *p == path));
        let row = panel.read_with(cx, |panel, _| panel.row_bounds(ix.expect("listed")));
        cx.simulate_click(row.expect("drawn").center(), Modifiers::none());
        draw(cx);
        let [left, right] = sides(&active_diff(&workbench, cx), cx);
        (buffer_text(&left, cx), buffer_text(&right, cx))
    };
    // Staged rows come first: the links, HEAD's target against the index's.
    assert_eq!(open_row("tracked.rs", cx), ("a.rs".into(), "b.rs".into()));
    let ext = open_row("ext.rs", cx);
    assert_eq!(ext, (one.display().to_string(), two.display().to_string()));

    // A refresh reads the link again.
    fs::remove_file(root.join("tracked.rs")).unwrap();
    symlink("c.rs", root.join("tracked.rs")).unwrap();
    git(root, &["add", "tracked.rs"]);
    open_row("tracked.rs", cx);
    dispatch("git.refresh", cx);
    let [left, right] = sides(&active_diff(&workbench, cx), cx);
    assert_eq!(
        (buffer_text(&left, cx), buffer_text(&right, cx)),
        ("a.rs".into(), "c.rs".into())
    );
}

#[gpui::test]
fn a_queued_save_and_stage_stops_if_its_editor_moves_to_another_file(cx: &mut TestAppContext) {
    let dir = fixture();
    let root = dir.path();
    let (workbench, _, cx) = open(cx, root);
    set_vim_enabled_in(cx);
    // Save As, then a rename: either way the editor edits another file.
    for (file, command, moved) in [
        ("a.rs", "Gwrite", "saved_as.rs"),
        ("c.rs", "Gitsigns stage_buffer", "renamed.rs"),
    ] {
        let tab = open_file(&workbench, root, file, cx);
        let before = (read(root, file), index_text(root, file));
        Workbench::hold_git_work_in(cx);
        let blocker = root.join("d.rs");
        workbench.update(cx, |workbench, cx| {
            workbench.stage_files(vec![blocker], true, cx)
        });
        draw(cx);
        assert_eq!(cx.update(|_, cx| Workbench::git_work_held(cx)), 1);
        keys("g g i x escape", cx);
        ex(command, cx);
        let moved = root.join(moved);
        tab.update(cx, |tab, _| tab.model_mut().set_path(Some(moved.clone())));
        cx.update(|_, cx| Workbench::release_git_work(0, cx));
        draw(cx);
        assert_eq!(
            cx.update(|_, cx| Workbench::git_work_held(cx)),
            0,
            ":{command}"
        );
        assert_eq!(
            vim_message(&tab, cx),
            Some(format!(
                "Not saved or staged: the editor no longer edits {file}"
            ))
        );
        assert_eq!(
            (read(root, file), index_text(root, file)),
            before,
            ":{command}"
        );
        assert!(!moved.exists(), ":{command}");
    }
}

#[gpui::test]
#[cfg(unix)]
fn staged_links_to_one_file_each_have_their_own_diff(cx: &mut TestAppContext) {
    use std::os::unix::fs::symlink;
    let dir = fixture();
    let root = dir.path();
    symlink("a.rs", root.join("link1.rs")).unwrap();
    symlink("a.rs", root.join("link2.rs")).unwrap();
    git(root, &["add", "link1.rs", "link2.rs"]);
    let (workbench, panel, cx) = open(cx, root);
    let open_row = |name: &str, cx: &mut VisualTestContext| {
        let path = root.join(name);
        let ix = panel.read_with(cx, |panel, _| panel.paths().iter().position(|p| *p == path));
        let row = panel.read_with(cx, |panel, _| panel.row_bounds(ix.expect("listed")));
        cx.simulate_click(row.expect("drawn").center(), Modifiers::none());
        draw(cx);
        let title = workbench.read_with(cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            pane.active_item().unwrap().tab_title(cx).to_string()
        });
        let [left, right] = sides(&active_diff(&workbench, cx), cx);
        (title, buffer_text(&left, cx), buffer_text(&right, cx))
    };
    let link = |name: &str| {
        (
            format!("{name} (HEAD ↔ Index)"),
            String::new(),
            "a.rs".into(),
        )
    };
    assert_eq!(open_row("link1.rs", cx), link("link1.rs"));
    assert_eq!(open_row("link2.rs", cx), link("link2.rs"));
    assert_eq!(
        open_row("a.rs", cx),
        (
            "a.rs (HEAD ↔ Index)".into(),
            "head\n".into(),
            "index\n".into()
        )
    );
    let tabs = workbench.read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len());
    assert_eq!(tabs, 3);
}

#[cfg(test)]
#[path = "git_panel_merge_tests.rs"]
mod merge;

#[cfg(test)]
#[path = "git_panel_merge_queue_tests.rs"]
mod merge_queue;
